use super::*;

#[test]
fn stopped_run_cannot_be_reopened_or_claimed_by_another_session() {
    let fake = Arc::new(crate::fake::FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake, gates::enabled_opts(100));
    broker.open_run("owner", "run").unwrap();
    assert!(broker.open_run("other", "run").is_err());
    broker.request_stop("run").unwrap();
    assert!(broker.open_run("owner", "run").is_err());
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
}

#[test]
fn timeout_and_new_observation_do_not_allow_a_second_native_action() {
    let fake = Arc::new(crate::fake::FakeAdapter::new());
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(30));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let generation = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    let first = broker.act(sample_click(
        "run",
        &target,
        generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "first",
    ));
    assert_eq!(first.kind, OutcomeKind::Unknown);
    assert!(matches!(
        broker.observe("run"),
        Err(BrokerError::LeaseHeld { .. })
    ));
    assert!(matches!(
        broker.observe_preview("run"),
        Err(BrokerError::LeaseHeld { .. })
    ));
    let second = broker.act(sample_click(
        "run",
        &target,
        generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "second",
    ));
    assert_eq!(second.kind, OutcomeKind::Rejected);
    assert!(fake.wait_for_execution("first", Duration::from_secs(2)));
    assert_eq!(fake.executions(), ["first"]);
    assert_eq!(
        broker.request_stop("run").unwrap(),
        StopState::StopRequested
    );
    assert!(broker.resume("run").is_err());
    fake.finish_in_flight();
    assert_eq!(
        broker.wait_stopped("run", Duration::from_secs(1)).unwrap(),
        StopState::Stopped
    );
}

#[test]
fn preview_does_not_replace_model_snapshot_or_clear_unknown_result() {
    let fake = Arc::new(crate::fake::FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let generation = broker.authorize_target("run", &target).unwrap();
    let model = broker.observe("run").unwrap();
    let preview = broker.observe_preview("run").unwrap();
    assert_ne!(preview.snapshot_id, model.snapshot_id);
    assert_eq!(broker.act_defaults("run").unwrap().2, model.snapshot_id);
    assert_eq!(
        broker
            .act(sample_click(
                "run",
                &target,
                generation,
                &model.snapshot_id,
                model.geometry_revision,
                "click"
            ))
            .kind,
        OutcomeKind::Verified
    );
}

#[test]
fn coordinates_and_element_handles_must_belong_to_observation() {
    let fake = Arc::new(crate::fake::FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    broker.open_run("owner", "run").unwrap();
    let target = fake.fixture_id();
    let generation = broker.authorize_target("run", &target).unwrap();
    let obs = broker.observe("run").unwrap();
    let mut request = sample_click(
        "run",
        &target,
        generation,
        &obs.snapshot_id,
        obs.geometry_revision,
        "invalid",
    );
    request.target = crate::protocol::ActionTarget::Element {
        element_ref: "guessed-child".into(),
    };
    assert_eq!(broker.act(request.clone()).kind, OutcomeKind::Rejected);
    request.target = crate::protocol::ActionTarget::Coord { x: 64.0, y: 0.0 };
    assert_eq!(broker.act(request).kind, OutcomeKind::Rejected);
    assert!(fake.executions().is_empty());
}

#[test]
fn turning_feature_off_cancels_authorization_and_never_auto_resumes() {
    let fake = Arc::new(crate::fake::FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), gates::enabled_opts(100));
    broker.open_run("owner", "run").unwrap();
    broker.authorize_target("run", &fake.fixture_id()).unwrap();
    broker.set_feature_enabled(false);
    broker.set_feature_enabled(true);
    assert_eq!(broker.stop_state("run").unwrap(), StopState::Stopped);
    assert!(broker.observe("run").is_err());
    assert!(broker.resume("run").is_err());
}

fn sample_click(
    run: &str,
    target: &str,
    gen: u64,
    snap: &str,
    geo: u64,
    id: &str,
) -> ActionRequest {
    ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: id.into(),
        run_id: run.into(),
        target_id: target.into(),
        target_generation: gen,
        snapshot_id: snap.into(),
        geometry_revision: geo,
        action: crate::protocol::ActionKind::Click,
        target: crate::protocol::ActionTarget::Element {
            element_ref: "n1".into(),
        },
        parameters: serde_json::json!({}),
    }
}

mod regression {
    use super::*;
    use crate::broker::gates::enabled_opts;
    use crate::fake::FakeAdapter;
    use crate::protocol::ActionKind;

    fn setup() -> (
        ComputerUseBroker,
        Arc<FakeAdapter>,
        String,
        u64,
        String,
        u64,
    ) {
        let fake = Arc::new(FakeAdapter::new());
        let adapter: Arc<dyn ComputerUseAdapter> = fake.clone();
        let broker = ComputerUseBroker::new(adapter, enabled_opts(400));
        broker.open_run("sess", "run-1").unwrap();
        let tid = fake.fixture_id().to_string();
        let gen = broker.authorize_target("run-1", &tid).unwrap();
        let obs = broker.observe("run-1").unwrap();
        (
            broker,
            fake,
            tid,
            gen,
            obs.snapshot_id,
            obs.geometry_revision,
        )
    }

    #[test]
    fn feature_flag_default_rejects_without_execution() {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), BrokerOptions::default());
        let err = broker.open_run("s", "r").unwrap_err();
        assert_eq!(err, BrokerError::FeatureDisabled);
        assert!(fake.executions().is_empty());
    }

    #[test]
    fn identity_mismatch_zero_execution() {
        let (broker, fake, tid, gen, snap, geo) = setup();
        let mut req = sample_click("run-1", &tid, gen, &snap, geo, "a1");
        req.target_generation = gen + 9;
        let out = broker.act(req);
        assert_eq!(out.kind, OutcomeKind::Rejected);
        assert!(!out.executed);
        assert!(fake.executions().is_empty());
        assert_eq!(fake.desktop_fallback(), 0);
    }

    #[test]
    fn snapshot_mismatch_zero_execution() {
        let (broker, fake, tid, gen, _snap, geo) = setup();
        let req = sample_click("run-1", &tid, gen, "stale-snap", geo, "a1");
        let out = broker.act(req);
        assert_eq!(out.kind, OutcomeKind::Rejected);
        assert!(!out.executed);
        assert!(fake.executions().is_empty());
    }

    #[test]
    fn dead_target_does_not_fallback_to_desktop() {
        let (broker, fake, tid, gen, snap, geo) = setup();
        fake.set_alive(false);
        let req = sample_click("run-1", &tid, gen, &snap, geo, "a1");
        let out = broker.act(req.clone());
        assert_eq!(out.kind, OutcomeKind::Rejected);
        assert!(!out.executed);
        assert!(out.reason.unwrap().contains("desktop fallback"));
        assert_eq!(fake.desktop_fallback(), 0);
        assert!(fake.executions().is_empty());
        fake.set_alive(true);
        let retry = broker.act(req);
        assert_eq!(retry.kind, OutcomeKind::Rejected);
        assert!(!retry.executed);
        assert!(fake.executions().is_empty());
    }

    #[test]
    fn exclusive_lease_across_two_runs() {
        let fake = Arc::new(FakeAdapter::new());
        let opts = enabled_opts(400);
        let path = opts.lease_path.clone();
        let a = ComputerUseBroker::new(fake.clone(), opts);
        let opts_b = BrokerOptions {
            lease_path: path,
            ..enabled_opts(400)
        };
        let b = ComputerUseBroker::new(fake.clone(), opts_b);
        a.open_run("s", "run-a").unwrap();
        b.open_run("s", "run-b").unwrap();
        let tid = fake.fixture_id().to_string();
        let ga = a.authorize_target("run-a", &tid).unwrap();
        let gb = b.authorize_target("run-b", &tid).unwrap();
        let oa = a.observe("run-a").unwrap();
        let ob = b.observe("run-b").unwrap();
        let mut click_a = sample_click(
            "run-a",
            &tid,
            ga,
            &oa.snapshot_id,
            oa.geometry_revision,
            "aa",
        );
        click_a.action = ActionKind::TypeText;
        click_a.parameters = serde_json::json!({"text": "hi"});
        let out_a = a.act(click_a);
        assert_eq!(out_a.kind, OutcomeKind::Verified);
        let mut click_b = sample_click(
            "run-b",
            &tid,
            gb,
            &ob.snapshot_id,
            ob.geometry_revision,
            "bb",
        );
        click_b.action = ActionKind::TypeText;
        click_b.parameters = serde_json::json!({"text": "no"});
        let out_b = b.act(click_b.clone());
        assert_eq!(out_b.kind, OutcomeKind::Rejected);
        assert!(!out_b.executed);
        assert!(out_b.reason.unwrap().contains("lease"));
        a.request_stop("run-a").unwrap();
        // A transport retry retains the original rejection even after the lease
        // is free. A deliberate fresh action must have a fresh actionId.
        let retry = b.act(click_b.clone());
        assert_eq!(retry.kind, OutcomeKind::Rejected);
        assert!(!retry.executed);
        click_b.action_id = "fresh".into();
        assert_eq!(b.act(click_b).kind, OutcomeKind::Verified);
    }

    #[test]
    fn stop_requested_is_not_stopped_until_idle() {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_hang(true);
        fake.set_abort_does_not_quiesce(true);
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(80));
        broker.open_run("s", "run-1").unwrap();
        let tid = fake.fixture_id().to_string();
        let gen = broker.authorize_target("run-1", &tid).unwrap();
        let obs = broker.observe("run-1").unwrap();
        let req = sample_click(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "slow",
        );
        let broker = Arc::new(broker);
        let b2 = broker.clone();
        let th = std::thread::spawn(move || b2.act(req));
        let until = Instant::now() + Duration::from_secs(2);
        while fake.adapter_in_flight() == 0 && Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(fake.adapter_in_flight() > 0);
        let st = broker.request_stop("run-1").unwrap();
        assert_eq!(st, StopState::StopRequested);
        assert_eq!(
            broker.stop_state("run-1").unwrap(),
            StopState::StopRequested
        );
        assert!(fake.abort_called() >= 1);
        assert!(!fake.is_idle("run-1"));
        fake.finish_in_flight();
        let waited = broker
            .wait_stopped("run-1", Duration::from_secs(1))
            .unwrap();
        assert_eq!(waited, StopState::Stopped);
        let _ = th.join();
        let late = sample_click(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after",
        );
        let out = broker.act(late);
        assert_eq!(out.kind, OutcomeKind::Rejected);
        assert!(!out.executed);
    }

    #[test]
    fn late_callback_ignored_after_generation_change() {
        let (broker, _fake, _tid, gen, _snap, _geo) = setup();
        broker.request_stop("run-1").unwrap();
        assert!(!broker.report_late("run-1", gen, "old-action"));
        assert!(broker.dropped_late() >= 1);
    }

    #[test]
    fn action_id_dedupes_without_second_execution() {
        let (broker, fake, tid, gen, snap, geo) = setup();
        let req = sample_click("run-1", &tid, gen, &snap, geo, "same");
        let a = broker.act(req.clone());
        let b = broker.act(req);
        assert_eq!(a.kind, OutcomeKind::Verified);
        assert_eq!(b.kind, OutcomeKind::Verified);
        assert_eq!(fake.executions(), vec!["same".to_string()]);
    }

    #[test]
    fn timeout_is_unknown_and_not_auto_replayed() {
        let fake = Arc::new(FakeAdapter::new());
        // Hold until explicit stop. A finite sleep can finish before the caller
        // is scheduled again and does not deterministically exercise a timeout.
        fake.set_hang(true);
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
        broker.open_run("s", "run-1").unwrap();
        let tid = fake.fixture_id().to_string();
        let gen = broker.authorize_target("run-1", &tid).unwrap();
        let obs = broker.observe("run-1").unwrap();
        let req = sample_click(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "slow",
        );
        let out = broker.act(req.clone());
        assert_eq!(out.kind, OutcomeKind::Unknown);
        assert!(out.executed);
        let again = broker.act(req);
        assert_eq!(again.kind, OutcomeKind::Unknown);
        assert!(fake.wait_for_execution("slow", Duration::from_secs(2)));
        assert_eq!(fake.executions(), ["slow"]);
        broker.request_stop("run-1").unwrap();
        assert_eq!(
            broker
                .wait_stopped("run-1", Duration::from_secs(2))
                .unwrap(),
            StopState::Stopped
        );
    }

    #[test]
    fn applied_verified_rejected_unknown_distinct() {
        let (broker, fake, tid, gen, snap, geo) = setup();
        fake.set_verify_ok(false);
        let applied = broker.act(sample_click("run-1", &tid, gen, &snap, geo, "u1"));
        assert_eq!(applied.kind, OutcomeKind::Applied);
        fake.set_verify_ok(true);
        let obs = broker.observe("run-1").unwrap();
        let verified = broker.act(sample_click(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "u2",
        ));
        assert_eq!(verified.kind, OutcomeKind::Verified);
        let rejected = broker.act(sample_click(
            "run-1",
            &tid,
            gen + 3,
            &obs.snapshot_id,
            obs.geometry_revision,
            "u3",
        ));
        assert_eq!(rejected.kind, OutcomeKind::Rejected);
        assert_ne!(applied.kind, verified.kind);
        assert_ne!(verified.kind, rejected.kind);
        assert_ne!(applied.kind, OutcomeKind::Unknown);
    }

    #[test]
    fn preview_hide_stops_periodic_but_observe_still_runs() {
        let (broker, fake, _tid, _gen, _snap, _geo) = setup();
        broker.set_preview_visible("run-1", true).unwrap();
        assert!(fake.periodic_preview_active());
        fake.drive_preview_tick();
        fake.drive_preview_tick();
        assert_eq!(fake.preview_ticks(), 2);
        broker.set_preview_visible("run-1", false).unwrap();
        assert!(!fake.periodic_preview_active());
        fake.drive_preview_tick();
        assert_eq!(fake.preview_ticks(), 2);
        let before = fake.observe_calls();
        broker.observe("run-1").unwrap();
        assert_eq!(fake.observe_calls(), before + 1);
    }

    #[test]
    fn fork_discards_snapshot_and_does_not_replay() {
        let (broker, fake, tid, gen, snap, geo) = setup();
        let _ = broker.act(sample_click("run-1", &tid, gen, &snap, geo, "orig"));
        broker.fork_run("run-1", "run-2").unwrap();
        let replay = broker.act(sample_click("run-2", &tid, gen, &snap, geo, "orig"));
        assert_eq!(replay.kind, OutcomeKind::Rejected);
        assert!(!replay.executed);
        assert_eq!(fake.executions(), vec!["orig".to_string()]);
    }

    #[test]
    fn observe_act_verify_reads_fixture_not_tool_ok() {
        let (broker, fake, tid, gen, snap, geo) = setup();
        fake.set_verify_ok(true);
        let (before, out, after) = broker
            .observe_act_verify(sample_click("run-1", &tid, gen, &snap, geo, "loop"))
            .expect("loop");
        assert_ne!(before.snapshot_id, after.snapshot_id);
        assert_eq!(out.kind, OutcomeKind::Verified);
        assert_eq!(fake.executions(), vec!["loop".to_string()]);
    }

    #[test]
    fn yolo_parameter_does_not_skip_authorization() {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").unwrap();
        let mut req = sample_click("run-1", "nope", 1, "s", 1, "y");
        req.parameters = serde_json::json!({"yolo": true, "acceptEdits": true});
        let out = broker.act(req);
        assert_eq!(out.kind, OutcomeKind::Rejected);
        assert!(!out.executed);
        assert!(fake.executions().is_empty());
    }
}
