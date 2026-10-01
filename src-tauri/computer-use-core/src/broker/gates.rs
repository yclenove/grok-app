//! Legacy fixture scenarios retained as regression coverage during the review.
use super::*;

pub(super) fn click_req(
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

pub(super) fn type_req(
    run: &str,
    target: &str,
    gen: u64,
    snap: &str,
    geo: u64,
    id: &str,
    text: &str,
) -> ActionRequest {
    let mut req = click_req(run, target, gen, snap, geo, id);
    req.action = crate::protocol::ActionKind::TypeText;
    req.parameters = serde_json::json!({ "text": text });
    req
}

pub(super) fn coord_req(
    run: &str,
    target: &str,
    gen: u64,
    snap: &str,
    geo: u64,
    id: &str,
) -> ActionRequest {
    let mut req = click_req(run, target, gen, snap, geo, id);
    req.target = crate::protocol::ActionTarget::Coord { x: 4.0, y: 4.0 };
    req
}

pub(super) fn enabled_opts(timeout_ms: u64) -> BrokerOptions {
    let instance_id = Uuid::new_v4().to_string();
    BrokerOptions {
        instance_id: instance_id.clone(),
        lease_path: std::env::temp_dir().join(format!(
            "grok-cu-gate-{}-{}.lease",
            std::process::id(),
            instance_id
        )),
        feature_enabled: true,
        action_timeout: Duration::from_millis(timeout_ms),
        ..BrokerOptions::default()
    }
}

/// Host-side gate runner used by `cu-probe` (not the lib test harness).
pub fn run_broker_gates() -> Result<(), String> {
    use crate::fake::FakeAdapter;
    use std::time::{Duration, Instant};

    // 1. Feature flag default off, zero execution.
    let fake = Arc::new(FakeAdapter::new());
    if BrokerOptions::default().feature_enabled {
        return Err("feature must default off".into());
    }
    ComputerUseBroker::new(fake.clone(), BrokerOptions::default())
        .open_run("s", "r")
        .err()
        .filter(|e| *e == BrokerError::FeatureDisabled)
        .ok_or("expected feature disabled")?;
    if !fake.executions().is_empty() {
        return Err("disabled flag executed".into());
    }
    println!("gate: feature_default_off");

    // 2. Identity mismatch + dead target, no desktop fallback, actionId dedupe.
    let opts = enabled_opts(400);
    let broker = ComputerUseBroker::new(fake.clone(), opts);
    broker
        .open_run("sess", "run-1")
        .map_err(|e| e.to_string())?;
    let tid = fake.fixture_id().to_string();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    let mut bad = click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "a1",
    );
    bad.target_generation = gen + 9;
    let out = broker.act(bad);
    if out.kind != OutcomeKind::Rejected || out.executed {
        return Err("identity mismatch must reject with zero execution".into());
    }
    fake.set_alive(false);
    let dead = broker.act(click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "dead",
    ));
    if dead.executed || fake.desktop_fallback() != 0 {
        return Err("dead target fell back to desktop".into());
    }
    fake.set_alive(true);
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    let first = broker.act(click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "same",
    ));
    let second = broker.act(click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "same",
    ));
    if first.kind != OutcomeKind::Verified || second.kind != OutcomeKind::Verified {
        return Err("dedupe should return verified".into());
    }
    if fake.executions() != vec!["same".to_string()] {
        return Err("actionId executed twice".into());
    }
    println!("gate: identity_dead_dedupe");

    // 3. Exclusive desktop lease across two runs.
    {
        let fake = Arc::new(FakeAdapter::new());
        let opts_a = enabled_opts(400);
        let path = opts_a.lease_path.clone();
        let a = ComputerUseBroker::new(fake.clone(), opts_a);
        let mut opts_b = enabled_opts(400);
        opts_b.lease_path = path;
        let b = ComputerUseBroker::new(fake.clone(), opts_b);
        a.open_run("s", "run-a").map_err(|e| e.to_string())?;
        b.open_run("s", "run-b").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let ga = a
            .authorize_target("run-a", &tid)
            .map_err(|e| e.to_string())?;
        let gb = b
            .authorize_target("run-b", &tid)
            .map_err(|e| e.to_string())?;
        let oa = a.observe("run-a").map_err(|e| e.to_string())?;
        let ob = b.observe("run-b").map_err(|e| e.to_string())?;
        let out_a = a.act(type_req(
            "run-a",
            &tid,
            ga,
            &oa.snapshot_id,
            oa.geometry_revision,
            "aa",
            "hi",
        ));
        if out_a.kind != OutcomeKind::Verified {
            return Err(format!("lease holder should verify, got {:?}", out_a.kind));
        }
        let out_b = b.act(type_req(
            "run-b",
            &tid,
            gb,
            &ob.snapshot_id,
            ob.geometry_revision,
            "bb",
            "no",
        ));
        if out_b.kind != OutcomeKind::Rejected || out_b.executed {
            return Err("second run stole the desktop lease".into());
        }
        if !out_b.reason.as_deref().unwrap_or("").contains("lease") {
            return Err(format!("expected lease reject, got {:?}", out_b.reason));
        }
        a.request_stop("run-a").map_err(|e| e.to_string())?;
        let _ = a.wait_stopped("run-a", Duration::from_millis(200));
        println!("gate: exclusive_lease");
    }

    // 4. stop_requested ≠ stopped until idle; post-stop dispatch rejected.
    {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_hang(true);
        fake.set_abort_does_not_quiesce(true);
        let broker = Arc::new(ComputerUseBroker::new(fake.clone(), enabled_opts(80)));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let req = click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "slow",
        );
        let b2 = broker.clone();
        let th = std::thread::spawn(move || b2.act(req));
        std::thread::sleep(Duration::from_millis(40));
        let st = broker.request_stop("run-1").map_err(|e| e.to_string())?;
        if st != StopState::StopRequested {
            return Err(format!("expected stop_requested, got {st:?}"));
        }
        if broker.stop_state("run-1").map_err(|e| e.to_string())? != StopState::StopRequested {
            return Err("stop_requested must not be stopped while adapter busy".into());
        }
        if fake.abort_called() < 1 {
            return Err("abort was not called".into());
        }
        if fake.is_idle("run-1") {
            return Err("Promise/abort return is not quiescence".into());
        }
        fake.finish_in_flight();
        let waited = broker
            .wait_stopped("run-1", Duration::from_secs(1))
            .map_err(|e| e.to_string())?;
        if waited != StopState::Stopped {
            return Err("expected stopped after idle".into());
        }
        let _ = th.join();
        let late = click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after",
        );
        let out = broker.act(late);
        if out.kind != OutcomeKind::Rejected || out.executed {
            return Err("post-stop dispatch must reject with zero execution".into());
        }
        println!("gate: stop_requested_vs_stopped");
    }

    // 5. Late callback after generation change is dropped.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        broker.request_stop("run-1").map_err(|e| e.to_string())?;
        if broker.report_late("run-1", gen, "old-action") {
            return Err("late callback must be ignored after generation change".into());
        }
        if broker.dropped_late() < 1 {
            return Err("dropped_late was not incremented".into());
        }
        println!("gate: late_callback_dropped");
    }

    // 6. Timeout → unknown, same actionId is not auto-replayed.
    {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_delay(Duration::from_millis(300));
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let req = click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "slow",
        );
        let out = broker.act(req.clone());
        if out.kind != OutcomeKind::Unknown || !out.executed {
            return Err(format!(
                "timeout must be unknown/executed, got {:?}",
                out.kind
            ));
        }
        let again = broker.act(req);
        if again.kind != OutcomeKind::Unknown {
            return Err("timeout must not auto-replay a new execution".into());
        }
        if fake.executions().len() != 1 {
            return Err(format!(
                "timeout auto-replayed, executions={}",
                fake.executions().len()
            ));
        }
        println!("gate: timeout_unknown_no_replay");
    }

    // 7. Preview hide stops periodic capture; explicit observe still runs.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        broker
            .set_preview_visible("run-1", true)
            .map_err(|e| e.to_string())?;
        if !fake.periodic_preview_active() {
            return Err("preview visible should start periodic capture".into());
        }
        fake.drive_preview_tick();
        fake.drive_preview_tick();
        broker
            .set_preview_visible("run-1", false)
            .map_err(|e| e.to_string())?;
        fake.drive_preview_tick();
        if fake.preview_ticks() != 2 {
            return Err("hidden preview must stop periodic ticks".into());
        }
        let before = fake.observe_calls();
        broker.observe("run-1").map_err(|e| e.to_string())?;
        if fake.observe_calls() != before + 1 {
            return Err("explicit observe must still run after preview hide".into());
        }
        println!("gate: preview_hide_vs_observe");
    }

    // 8. fork does not replay stored input.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let _ = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "orig",
        ));
        broker
            .fork_run("run-1", "run-2")
            .map_err(|e| e.to_string())?;
        let replay = broker.act(click_req(
            "run-2",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "orig",
        ));
        if replay.kind != OutcomeKind::Rejected || replay.executed {
            return Err("fork must not replay historical input".into());
        }
        if fake.executions() != vec!["orig".to_string()] {
            return Err("fork replayed input events".into());
        }
        if broker.recovery_source("run-2") != Some(RecoverySource::System) {
            return Err("fork recovery must be system".into());
        }
        println!("gate: fork_no_replay");
    }

    // 9. Traces record observe/act/stop; preview ticks stay UI-only.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        broker
            .set_preview_visible("run-1", true)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let _ = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "tr1",
        ));
        broker.request_stop("run-1").map_err(|e| e.to_string())?;
        let model = broker.traces(Some("run-1"), true);
        let kinds: Vec<_> = model.iter().map(|t| t.kind.as_str()).collect();
        for need in ["open", "authorize", "observe", "act", "stop_requested"] {
            if !kinds.contains(&need) {
                return Err(format!("missing model trace {need}: {kinds:?}"));
            }
        }
        if kinds.iter().any(|k| k.starts_with("preview_")) {
            return Err("preview traces leaked into model content".into());
        }
        let ui = broker.traces(Some("run-1"), false);
        if !ui
            .iter()
            .any(|t| t.kind == "preview_shown" && t.audience == TraceAudience::Ui)
        {
            return Err("preview shown must be a UI trace".into());
        }
        println!("gate: traces_model_vs_ui");
    }

    // 10. Stop/deny is not auto-resumed; reconnect does not restore snapshot.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let _ = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "r1",
        ));
        broker.request_stop("run-1").map_err(|e| e.to_string())?;
        if broker.resume("run-1").is_ok() {
            return Err("stopped run must not auto-resume".into());
        }
        if broker.reconnect("run-1").is_ok() {
            return Err("stopped run must not reconnect-continue".into());
        }
        broker
            .open_run("s", "run-live")
            .map_err(|e| e.to_string())?;
        let gen = broker
            .authorize_target("run-live", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-live").map_err(|e| e.to_string())?;
        let snap = obs.snapshot_id.clone();
        broker.reconnect("run-live").map_err(|e| e.to_string())?;
        if broker.recovery_source("run-live") != Some(RecoverySource::System) {
            return Err("reconnect recovery must be system".into());
        }
        let stale = broker.act(click_req(
            "run-live",
            &tid,
            gen,
            &snap,
            obs.geometry_revision,
            "stale",
        ));
        if stale.kind != OutcomeKind::Rejected || stale.executed {
            return Err("reconnect must drop the old snapshot".into());
        }
        println!("gate: stop_no_autoresume_reconnect");
    }

    // 11. Unknown requires a fresh observe; same actionId is still not replayed.
    {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_delay(Duration::from_millis(300));
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(40));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let req = click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "slow",
        );
        let out = broker.act(req.clone());
        if out.kind != OutcomeKind::Unknown {
            return Err(format!("expected unknown, got {:?}", out.kind));
        }
        let again = broker.act(req);
        if again.kind != OutcomeKind::Unknown || fake.executions().len() != 1 {
            return Err("unknown same actionId must not replay".into());
        }
        let next = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "next",
        ));
        if next.kind != OutcomeKind::Rejected || next.executed {
            return Err("unknown must observe before a new action".into());
        }
        if !next.reason.as_deref().unwrap_or("").contains("observe") {
            return Err(format!("expected observe-first, got {:?}", next.reason));
        }
        println!("gate: unknown_must_observe");
    }

    // 12. Coordinate action without visual observation is rejected.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let blind = broker.act(coord_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "blind",
        ));
        if blind.kind != OutcomeKind::Rejected || blind.executed {
            return Err("coord click without image must reject".into());
        }
        fake.set_include_png(true);
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let seen = broker.act(coord_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "seen",
        ));
        if seen.kind != OutcomeKind::Verified || !seen.executed {
            return Err(format!("coord with image should verify, got {:?}", seen));
        }
        println!("gate: no_vision_no_coord");
    }

    // 13. Concurrent desktop writes on one run are rejected.
    {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_hang(true);
        let broker = Arc::new(ComputerUseBroker::new(fake.clone(), enabled_opts(800)));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let first = type_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "aa",
            "hi",
        );
        let b2 = broker.clone();
        let th = std::thread::spawn(move || b2.act(first));
        let wait_start = Instant::now();
        while fake.adapter_in_flight() == 0 {
            if wait_start.elapsed() > Duration::from_secs(1) {
                fake.finish_in_flight();
                let _ = th.join();
                return Err("first desktop write never started".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let second = broker.act(type_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "bb",
            "no",
        ));
        if second.kind != OutcomeKind::Rejected || second.executed {
            return Err("second desktop write ran concurrently".into());
        }
        fake.finish_in_flight();
        let _ = th.join();
        println!("gate: concurrent_desktop_write");
    }

    // 14. observe→act→verify timings are separate buckets, not a summed total.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id().to_string();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let (before, out, after) = broker
            .observe_act_verify(click_req(
                "run-1",
                &tid,
                gen,
                &obs.snapshot_id,
                obs.geometry_revision,
                "loop",
            ))
            .map_err(|e| e.to_string())?;
        if before.snapshot_id == after.snapshot_id {
            return Err("verify observe must be a new snapshot".into());
        }
        if out.kind != OutcomeKind::Verified {
            return Err(format!("loop should verify, got {:?}", out.kind));
        }
        let t = broker.timings("run-1").map_err(|e| e.to_string())?;
        let encoded = serde_json::to_string(&t).map_err(|e| e.to_string())?;
        if !encoded.contains("observeMs")
            || !encoded.contains("actMs")
            || !encoded.contains("verifyMs")
        {
            return Err(format!("timings missing buckets: {encoded}"));
        }
        if encoded.contains("totalMs") || encoded.contains("total_ms") {
            return Err("timings must not expose a summed total".into());
        }
        let kinds: Vec<_> = broker
            .traces(Some("run-1"), true)
            .into_iter()
            .map(|e| e.kind)
            .collect();
        if !kinds.iter().any(|k| k == "verify") {
            return Err("loop missing verify trace".into());
        }
        println!("gate: observe_act_verify_timings");
    }

    // 15. Identity rotation: old target id is dead, no desktop fallback.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        fake.rotate_identity();
        let out = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "stale-id",
        ));
        if out.kind != OutcomeKind::Rejected || out.executed {
            return Err(format!("stale identity must reject, got {out:?}"));
        }
        if fake.desktop_fallback() != 0 {
            return Err("identity rotation fell back to desktop".into());
        }
        if fake.executions().iter().any(|id| id == "stale-id") {
            return Err("stale identity executed".into());
        }
        println!("gate: identity_stamp_no_reuse");
    }

    // 16. Host control surface is not listable or authorizable.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let host = fake.host_id();
        let listed = broker.list_targets("run-1").map_err(|e| e.to_string())?;
        if listed.iter().any(|t| t.target_id == host) {
            return Err("host control surface leaked into list_targets".into());
        }
        match broker.authorize_target("run-1", &host) {
            Err(BrokerError::TargetUnauthorized) | Err(BrokerError::DeadTarget) => {}
            other => {
                return Err(format!("host authorize must fail, got {other:?}"));
            }
        }
        if !fake.executions().is_empty() {
            return Err("host surface executed".into());
        }
        println!("gate: host_surface_excluded");
    }

    // 17. Session lock pauses the run; no dispatch until resume + observe.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        fake.set_session_locked(true);
        let locked = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "during-lock",
        ));
        if locked.kind != OutcomeKind::Rejected || locked.executed {
            return Err(format!("lock must reject, got {locked:?}"));
        }
        if fake.executions().iter().any(|id| id == "during-lock") {
            return Err("lock-screen action executed".into());
        }
        fake.set_session_locked(false);
        let still = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after-unlock-no-resume",
        ));
        if still.kind != OutcomeKind::Rejected || still.executed {
            return Err("unlock without resume must stay paused".into());
        }
        broker.resume("run-1").map_err(|e| e.to_string())?;
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let ok = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after-resume",
        ));
        if ok.kind != OutcomeKind::Verified || !ok.executed {
            return Err(format!("resume after lock should run, got {ok:?}"));
        }
        println!("gate: session_lock_pauses");
    }

    // 18. Clipboard restore uses sequence numbers; user copies win.
    {
        use crate::clipboard::should_restore;
        if !should_restore(10, 11, 1) {
            return Err("single host write must restore".into());
        }
        if should_restore(10, 12, 1) {
            return Err("user copy during paste must not restore".into());
        }
        println!("gate: clipboard_sequence_restore");
    }

    // R1. Focus drift pauses foreground dispatch; no execution.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        fake.set_focus_drifted(true);
        let drifted = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "during-drift",
        ));
        if drifted.kind != OutcomeKind::Rejected || drifted.executed {
            return Err(format!("focus drift must reject, got {drifted:?}"));
        }
        if fake.executions().iter().any(|id| id == "during-drift") {
            return Err("focus-drift action executed".into());
        }
        fake.set_focus_drifted(false);
        let still = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after-refocus-no-resume",
        ));
        if still.kind != OutcomeKind::Rejected || still.executed {
            return Err("refocus without resume must stay paused".into());
        }
        broker.resume("run-1").map_err(|e| e.to_string())?;
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let ok = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after-focus-resume",
        ));
        if ok.kind != OutcomeKind::Verified || !ok.executed {
            return Err(format!("resume after focus should run, got {ok:?}"));
        }
        println!("gate: focus_drift_pauses");
    }

    // R2. Topology / DPI epoch rejects stale geometry; no desktop fallback.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        fake.bump_geometry();
        let stale = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "stale-geo",
        ));
        if stale.kind != OutcomeKind::Rejected || stale.executed {
            return Err(format!("stale geometry must reject, got {stale:?}"));
        }
        if fake.executions().iter().any(|id| id == "stale-geo") {
            return Err("stale geometry executed".into());
        }
        if fake.desktop_fallback() != 0 {
            return Err("geometry invalidation fell back to desktop".into());
        }
        println!("gate: topology_invalidates_geometry");
    }

    // R5. Fake multi-monitor negative origin + DPI mapping.
    {
        let fake = Arc::new(FakeAdapter::new());
        fake.set_origin(-1920, 108);
        fake.set_dpi_scale(1.25);
        let mapped = fake.virtual_from_image(10.0, 8.0);
        if mapped != (-1907, 118) {
            return Err(format!("virtual map expected (-1907,118) got {mapped:?}"));
        }
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let ok = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "neg-coord",
        ));
        if ok.kind != OutcomeKind::Verified || !ok.executed {
            return Err(format!("mapped click should run, got {ok:?}"));
        }
        if fake.last_virtual() != Some((-1880, 128)) {
            return Err(format!(
                "element click virtual {:?} (origin -1920,108 @1.25 on 32,16)",
                fake.last_virtual()
            ));
        }
        fake.set_origin(0, 0);
        let stale = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after-origin-change",
        ));
        if stale.executed || fake.desktop_fallback() != 0 {
            return Err(format!("origin change must not execute, got {stale:?}"));
        }
        println!("gate: fake_multimonitor_geometry");
    }

    // R7. User input / takeover pauses and stops preview capture.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        broker
            .set_preview_visible("run-1", true)
            .map_err(|e| e.to_string())?;
        if !fake.periodic_preview_active() {
            return Err("preview should start".into());
        }
        fake.set_user_input_active(true);
        let blocked = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "during-user",
        ));
        if blocked.executed || fake.executions().iter().any(|id| id == "during-user") {
            return Err("user input must not dispatch".into());
        }
        if fake.periodic_preview_active() {
            return Err("user input must stop preview capture".into());
        }
        fake.set_user_input_active(false);
        broker.resume("run-1").map_err(|e| e.to_string())?;
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        broker
            .set_preview_visible("run-1", true)
            .map_err(|e| e.to_string())?;
        broker.takeover("run-1").map_err(|e| e.to_string())?;
        let after = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "during-takeover",
        ));
        if after.executed || fake.periodic_preview_active() {
            return Err("takeover must pause and stop capture".into());
        }
        println!("gate: user_input_takeover_pauses");
    }

    // R8. Restore re-validates target; traces survive a new broker (process restart).
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
        let tid = fake.fixture_id();
        let gen = broker
            .authorize_target("run-1", &tid)
            .map_err(|e| e.to_string())?;
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let ok = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "pre-restart",
        ));
        if !ok.executed {
            return Err(format!("seed act failed {ok:?}"));
        }
        let path = std::env::temp_dir().join(format!(
            "cu-traces-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        broker
            .persist_run_to_path("run-1", &path)
            .map_err(|e| e.to_string())?;
        let fake2 = Arc::new(FakeAdapter::new());
        let broker2 = ComputerUseBroker::new(fake2.clone(), enabled_opts(400));
        broker2
            .restore_run_from_path(&path)
            .map_err(|e| e.to_string())?;
        let traces = broker2.traces(Some("run-1"), true);
        if traces.is_empty() {
            return Err("traces missing after restore".into());
        }
        let unauthorized = broker2.act(click_req(
            "run-1",
            &tid,
            1,
            "snap-old",
            1,
            "after-restore-no-auth",
        ));
        if unauthorized.executed {
            return Err("restore must not keep authorization".into());
        }
        fake2.set_alive(false);
        match broker2.authorize_target("run-1", &fake2.fixture_id()) {
            Err(_) => {}
            Ok(_) => return Err("dead target must not re-authorize after restore".into()),
        }
        fake2.set_alive(true);
        let gen = broker2
            .authorize_target("run-1", &fake2.fixture_id())
            .map_err(|e| e.to_string())?;
        let obs = broker2.observe("run-1").map_err(|e| e.to_string())?;
        let again = broker2.act(click_req(
            "run-1",
            &fake2.fixture_id(),
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            "after-reauth",
        ));
        if again.kind != OutcomeKind::Verified || !again.executed {
            return Err(format!("re-auth after restore should run, got {again:?}"));
        }
        let _ = std::fs::remove_file(path);
        println!("gate: restore_revalidates_and_traces");
    }

    // R3. Surface classification (Host inject uses the same helper).
    {
        use crate::surface::{classify_surface, surface_allows_local, ComputerUseSurface};
        if surface_allows_local(ComputerUseSurface::RemoteIm)
            || surface_allows_local(ComputerUseSurface::Scheduled)
            || surface_allows_local(ComputerUseSurface::Ssh)
        {
            return Err("non-local surfaces must not inherit Computer Use".into());
        }
        if classify_surface(true, true, false) != ComputerUseSurface::Scheduled {
            return Err("scheduled must win over ssh".into());
        }
        println!("gate: surface_no_inherit");
    }

    // R9. Managed browser navigate/download through the Host Broker.
    {
        let fake = Arc::new(FakeAdapter::new());
        let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
        broker
            .open_run("sess-a", "run-a")
            .map_err(|e| e.to_string())?;
        if broker
            .browser_navigate("run-a", "tab-nav", "https://example.test/b", "nav-1")
            .is_ok()
        {
            return Err("broker navigate must not succeed without a worker".into());
        }
        let root = std::env::temp_dir().join(format!("cu-broker-dl-{}", uuid::Uuid::new_v4()));
        let worker = Arc::new(crate::browser::RecordingBrowserWorker::new(root.clone()));
        broker.tabs().set_profile_root(root.join("profiles"));
        broker.tabs().set_worker(worker.clone());
        broker.tabs().set_staging_root(root.clone());
        let tab = broker
            .tabs()
            .open_managed_profile("sess-a", "run-a", "nav")
            .map_err(|e| e.to_string())?
            .tab_id;
        if broker
            .browser_navigate("run-a", "other-tab", "https://example.test/b", "nav-2")
            .is_ok()
        {
            return Err("navigate must re-check tab identity".into());
        }
        let moved = broker
            .browser_navigate("run-a", &tab, "https://example.test/b", "nav-3")
            .map_err(|e| e.to_string())?;
        if moved.url != "https://example.test/b" {
            return Err(format!("broker navigate url {}", moved.url));
        }
        if worker.gotos.lock().is_empty() {
            return Err("broker navigate must invoke the managed worker".into());
        }
        match broker.browser_download(
            "run-a",
            &tab,
            "ok.bin",
            Some("C:\\\\temp\\\\evil.bin"),
            "dl-evil",
            "",
            None,
        ) {
            Err(_) => {}
            Ok(_) => return Err("broker download trusted a model path".into()),
        }
        let obs = broker
            .tabs()
            .observe_managed("run-a", "nav")
            .map_err(|e| e.to_string())?;
        let element = obs
            .nodes
            .iter()
            .find(|n| !n.element_ref.is_empty())
            .map(|n| n.element_ref.clone())
            .ok_or_else(|| "download fixture missing elementRef".to_string())?;
        let dest = broker
            .browser_download(
                "run-a",
                &tab,
                "ok.bin",
                None,
                "dl-ok",
                &obs.snapshot_id,
                Some(element.as_str()),
            )
            .map_err(|e| e.to_string())?;
        let body = std::fs::read_to_string(&dest).unwrap_or_default();
        if !body.starts_with("worker:") {
            return Err(format!(
                "broker download must come from the worker, got {body:?}"
            ));
        }
        if *worker.downloads.lock() == 0 {
            return Err("broker download must invoke the managed worker".into());
        }
        let _ = std::fs::remove_dir_all(root);
        println!("gate: broker_browser_navigate_download");
    }

    Ok(())
}
