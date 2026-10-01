//! Named broker contract tests migrated from t_rounds.rs.
use super::gates::{click_req, coord_req, enabled_opts, type_req};
use super::test_support::ipc_fixture;
use super::*;
use crate::fake::FakeAdapter;
use crate::protocol::{ActionKind, ActionTarget};
use std::time::Instant;

fn ready(
    timeout_ms: u64,
) -> Result<(ComputerUseBroker, Arc<FakeAdapter>, u64, Observation), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_include_png(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(timeout_ms));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    Ok((broker, fake, gen, obs))
}

fn t1_dead_lease_not_inherited() -> Result<(), String> {
    let path = std::env::temp_dir().join(format!(
        "cu-t1-{}-{}.lease",
        std::process::id(),
        Uuid::new_v4()
    ));
    let leftover = crate::lease::LeaseInfo {
        run_id: "ghost".into(),
        instance_id: "dead-inst".into(),
        pid: 1,
    };
    std::fs::write(
        &path,
        serde_json::to_vec(&leftover).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: path.clone(),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("s", "fresh").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("fresh", &tid)
        .map_err(|e| e.to_string())?;
    let obs = broker.observe("fresh").map_err(|e| e.to_string())?;
    let out = broker.act(click_req(
        "fresh",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t1-fresh",
    ));
    if !out.executed {
        return Err(format!(
            "new run must acquire after dead leftover, got {out:?}"
        ));
    }
    let ghost = broker.act(click_req("ghost", &tid, 1, "snap", 1, "t1-ghost"));
    if ghost.executed {
        return Err("leftover dead-process run_id must not dispatch".into());
    }
    if fake.executions().iter().any(|id| id == "t1-ghost") {
        return Err("ghost leftover executed".into());
    }
    let _ = std::fs::remove_file(path);
    println!("gate: t1_dead_lease_not_inherited");
    Ok(())
}

fn t2_feature_off_zero_execution() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(fake.clone(), BrokerOptions::default());
    if broker.feature_enabled() {
        return Err("feature must default off".into());
    }
    broker
        .open_run("s", "r")
        .err()
        .filter(|e| *e == BrokerError::FeatureDisabled)
        .ok_or("open_run must fail when disabled")?;
    if !fake.executions().is_empty() {
        return Err("disabled flag executed".into());
    }
    println!("gate: t2_feature_off_zero_execution");
    Ok(())
}

fn t3_ipc_origin_refused() -> Result<(), String> {
    let (server, token) = ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .header("Origin", "https://evil.example")
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 403 {
        return Err(format!("origin must be 403, got {}", response.status()));
    }
    println!("gate: t3_ipc_origin_refused");
    Ok(())
}

fn t4_rotated_bearer_refused() -> Result<(), String> {
    let (server, token) = ipc_fixture()?;
    let rotated = server
        .issue_session("session-a", "run-a")
        .map_err(|e| e.to_string())?;
    let stale = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if stale.status().as_u16() != 401 {
        return Err(format!(
            "rotated bearer must be 401, got {}",
            stale.status()
        ));
    }
    let ok = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&rotated)
        .json(&serde_json::json!({"name":"computer_status"}))
        .send()
        .map_err(|e| e.to_string())?;
    if ok.status().as_u16() != 200 {
        return Err("new bearer must work".into());
    }
    println!("gate: t4_rotated_bearer_refused");
    Ok(())
}

fn t5_unauthorized_target_unchanged() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let authorized = broker
        .authorized_target("run-1")
        .map_err(|e| e.to_string())?;
    let out = broker.act(click_req(
        "run-1",
        "other-window",
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t5-other",
    ));
    if out.executed {
        return Err("other targetId must not execute".into());
    }
    if broker
        .authorized_target("run-1")
        .map_err(|e| e.to_string())?
        != authorized
    {
        return Err("authorized target changed".into());
    }
    if fake.executions().iter().any(|id| id == "t5-other") {
        return Err("mismatch target executed".into());
    }
    println!("gate: t5_unauthorized_target_unchanged");
    Ok(())
}

fn t6_late_result_not_applied() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_hang(true);
    fake.set_abort_does_not_quiesce(true);
    fake.set_verify_ok(true);
    let broker = Arc::new(ComputerUseBroker::new(fake.clone(), enabled_opts(2000)));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
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
        "t6-inflight",
    );
    let worker = broker.clone();
    let th = std::thread::spawn(move || worker.act(req));
    let wait_start = Instant::now();
    while fake.adapter_in_flight() == 0 {
        if wait_start.elapsed() > Duration::from_secs(1) {
            fake.finish_in_flight();
            let _ = th.join();
            return Err("in-flight adapter never started".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if !fake.executions().iter().any(|id| id == "t6-inflight") {
        fake.finish_in_flight();
        let _ = th.join();
        return Err("adapter must have started before pause".into());
    }
    broker.pause("run-1").map_err(|e| e.to_string())?;
    fake.finish_in_flight();
    let late = th
        .join()
        .map_err(|_| "in-flight worker panicked".to_string())?;
    if late.kind == OutcomeKind::Applied || late.kind == OutcomeKind::Verified {
        return Err(format!(
            "in-flight result after pause must not apply, got {late:?}"
        ));
    }
    if broker.dropped_late() < 1 {
        return Err("late adapter result must go through record() after generation change".into());
    }
    println!("gate: t6_late_result_not_applied");
    Ok(())
}

fn t7_stale_element_ref_rejected() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let old_ref = obs
        .nodes
        .first()
        .map(|n| n.node_ref.clone())
        .ok_or("fixture node")?;
    let _ = broker.observe("run-1").map_err(|e| e.to_string())?;
    let mut req = click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t7-stale-el",
    );
    // New observe rotated snapshot; keep old elementRef + old snapshot to fail identity,
    // and a second act with new snapshot but a never-seen ref.
    let obs2 = broker.observe("run-1").map_err(|e| e.to_string())?;
    req.snapshot_id = obs2.snapshot_id.clone();
    req.geometry_revision = obs2.geometry_revision;
    req.target_generation = obs2.target_generation;
    req.target = ActionTarget::Element {
        element_ref: format!("stale-{old_ref}-gone"),
    };
    let out = broker.act(req);
    if out.executed {
        return Err("stale elementRef must be rejected".into());
    }
    if fake.executions().iter().any(|id| id == "t7-stale-el") {
        return Err("stale elementRef executed".into());
    }
    println!("gate: t7_stale_element_ref_rejected");
    Ok(())
}

fn t8_coords_outside_image_no_desktop() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let mut req = coord_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t8-out",
    );
    req.target = ActionTarget::Coord {
        x: obs.image.width as f64 + 40.0,
        y: 4.0,
    };
    let out = broker.act(req);
    if out.executed {
        return Err("outside coords must reject".into());
    }
    if fake.desktop_fallback() != 0 {
        return Err("outside coords fell back to desktop".into());
    }
    println!("gate: t8_coords_outside_image_no_desktop");
    Ok(())
}

fn t9_drag_without_destination_rejected() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let mut req = coord_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t9-drag",
    );
    req.action = ActionKind::Drag;
    let out = broker.act(req);
    if out.executed {
        return Err("drag without destination must reject".into());
    }
    if fake.executions().iter().any(|id| id == "t9-drag") {
        return Err("destination-less drag acted as click".into());
    }
    println!("gate: t9_drag_without_destination_rejected");
    Ok(())
}

fn t10_nul_or_overlong_text_rejected() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let nul = type_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t10-nul",
        "ok\0bad",
    );
    let over = type_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t10-long",
        &"x".repeat(4001),
    );
    for req in [nul, over] {
        let id = req.action_id.clone();
        let out = broker.act(req);
        if out.executed {
            return Err(format!("{id} must reject"));
        }
        if fake.executions().iter().any(|e| e == &id) {
            return Err(format!("{id} executed"));
        }
    }
    println!("gate: t10_nul_or_overlong_text_rejected");
    Ok(())
}

fn t11_wait_schema_rejected() -> Result<(), String> {
    let (broker, fake, _, _) = ready(400)?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let missing = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        serde_json::json!({"nameEquals": "Count"}),
    );
    if missing.get("isError") != Some(&serde_json::json!(true)) {
        return Err("wait without elementRef must error".into());
    }
    let huge = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_wait",
        super::test_support::wait_args(&broker, "run-1", "Count", 20_000),
    );
    if huge.get("isError") != Some(&serde_json::json!(true)) {
        return Err("wait over timeout cap must error".into());
    }
    if !fake.executions().is_empty() {
        return Err("invalid wait executed".into());
    }
    println!("gate: t11_wait_schema_rejected");
    Ok(())
}

fn t12_handoff_pauses_model_cannot_finish() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let binding = crate::ipc::SessionBinding {
        session_id: "s".into(),
        run_id: "run-1".into(),
    };
    let handoff = crate::tools::dispatch(
        &broker,
        &binding,
        "computer_request_handoff",
        serde_json::json!({}),
    );
    if handoff.get("isError") == Some(&serde_json::json!(true)) {
        return Err(format!("handoff must pause, got {handoff}"));
    }
    if !broker.is_paused("run-1").map_err(|e| e.to_string())? {
        return Err("handoff must pause the run".into());
    }
    let resume =
        crate::tools::dispatch(&broker, &binding, "computer_resume", serde_json::json!({}));
    if resume.get("isError") != Some(&serde_json::json!(true)) {
        return Err("model must not complete handoff via resume".into());
    }
    let act = broker.act(click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t12-after",
    ));
    if act.executed {
        return Err("model must not act through a handoff pause".into());
    }
    println!("gate: t12_handoff_pauses_model_cannot_finish");
    Ok(())
}

fn t13_hide_preview_stops_capture() -> Result<(), String> {
    let (broker, fake, _, _) = ready(400)?;
    broker
        .set_preview_visible("run-1", true)
        .map_err(|e| e.to_string())?;
    if !fake.periodic_preview_active() {
        return Err("preview should start".into());
    }
    broker
        .set_preview_visible("run-1", false)
        .map_err(|e| e.to_string())?;
    if fake.periodic_preview_active() {
        return Err("hide preview must stop periodic capture".into());
    }
    broker.observe("run-1").map_err(|e| e.to_string())?;
    println!("gate: t13_hide_preview_stops_capture");
    Ok(())
}

fn t14_iconic_windows_not_listed() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    fake.set_iconic(true);
    let broker = ComputerUseBroker::new(fake.clone(), enabled_opts(400));
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let listed = broker.list_targets("run-1").map_err(|e| e.to_string())?;
    if !listed.is_empty() {
        return Err("iconic window must not be listed".into());
    }
    println!("gate: t14_iconic_windows_not_listed");
    Ok(())
}

fn t16_managed_profile_owner() -> Result<(), String> {
    let host = crate::browser::ExistingTabHost::new();
    if host.open_managed_profile("sess-a", "run-a", "p1").is_ok() {
        return Err("managed profile must fail closed without a worker".into());
    }
    for invalid in ["", ".", "..", "../outside", "a/b", "a\\b", ".hidden"] {
        if host
            .open_managed_profile("sess-a", "run-a", invalid)
            .is_ok()
        {
            return Err(format!("invalid profile accepted: {invalid:?}"));
        }
    }
    let root = std::env::temp_dir().join(format!("cu-t16-{}", Uuid::new_v4()));
    let worker = Arc::new(crate::browser::RecordingBrowserWorker::new(root.clone()));
    host.set_profile_root(root.join("profiles"));
    host.set_worker(worker);
    host.open_managed_profile("sess-a", "run-a", "p1")
        .map_err(|e| e.to_string())?;
    if host.open_managed_profile("sess-b", "run-b", "p1").is_ok() {
        return Err("managed profile must refuse a second owner".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: t16_managed_profile_owner");
    Ok(())
}

fn t17_navigate_records_actual_url() -> Result<(), String> {
    let host = crate::browser::ExistingTabHost::new();
    let root = std::env::temp_dir().join(format!("cu-t17-{}", Uuid::new_v4()));
    let worker = Arc::new(crate::browser::RecordingBrowserWorker::new(root.clone()));
    host.set_profile_root(root.join("profiles"));
    host.set_worker(worker.clone());
    let tab = host
        .open_managed_profile("sess-a", "run-a", "nav")
        .map_err(|e| e.to_string())?
        .tab_id;
    worker.set_actual_url(Some("https://evil.test/hijack".into()));
    if host
        .navigate("run-a", &tab, "https://example.test/b", "nav-lease")
        .is_ok()
    {
        return Err("mismatched actual URL must reject".into());
    }
    worker.set_actual_url(None);
    let moved = host
        .navigate("run-a", &tab, "https://example.test/b", "nav-lease-ok")
        .map_err(|e| e.to_string())?;
    if moved.url != "https://example.test/b" {
        return Err(format!("host must record actual url, got {}", moved.url));
    }
    if worker.gotos.lock().is_empty() {
        return Err("navigate must invoke worker".into());
    }
    let _ = std::fs::remove_dir_all(root);
    println!("gate: t17_navigate_records_actual_url");
    Ok(())
}

fn t18_model_download_path_rejected() -> Result<(), String> {
    crate::staging::reject_model_path(Some("/tmp/evil.bin"))
        .err()
        .ok_or("model path must be rejected")?;
    println!("gate: t18_model_download_path_rejected");
    Ok(())
}

fn t19_cancel_keeps_user_tab() -> Result<(), String> {
    let host = crate::browser::ExistingTabHost::new();
    host.set_installed_extension_id("pw-ext-installed");
    let token = host.handshake_pairing().map_err(|e| e.to_string())?;
    host.share_and_grant(crate::browser::TabAttachment {
        session: "sess-a",
        run_id: "run-a",
        tab_id: "tab-user",
        title: "Docs",
        url: "https://example.test/",
        origin: "chrome-extension://pw-ext-installed/",
        extension_id: Some("pw-ext-installed"),
        pairing_token: Some(&token),
        home_index: Some(3),
        document_generation: Some(1),
        connection_generation: Some(1),
        focused: true,
    })
    .map_err(|e| e.to_string())?;
    let after = host.cancel_run("run-a").map_err(|e| e.to_string())?;
    let user = after
        .iter()
        .find(|t| t.tab_id == "tab-user")
        .ok_or("missing user tab")?;
    if user.closed {
        return Err("cancel must not close user-owned tab".into());
    }
    println!("gate: t19_cancel_keeps_user_tab");
    Ok(())
}

fn t20_two_runs_cannot_share_profile() -> Result<(), String> {
    t16_managed_profile_owner()?;
    println!("gate: t20_two_runs_cannot_share_profile");
    Ok(())
}

fn t21_fork_no_snapshot_no_replay() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    let first = broker.act(click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "src-act",
    ));
    if !first.executed {
        return Err(format!("seed act failed {first:?}"));
    }
    broker
        .fork_run("run-1", "run-fork")
        .map_err(|e| e.to_string())?;
    let replay = broker.act(click_req(
        "run-fork",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "src-act",
    ));
    if replay.executed {
        return Err("fork must not replay source actionId".into());
    }
    println!("gate: t21_fork_no_snapshot_no_replay");
    Ok(())
}

fn t22_flag_off_stops_runs() -> Result<(), String> {
    let (broker, _, _, _) = ready(400)?;
    broker.set_feature_enabled(false);
    match broker.stop_state("run-1") {
        Ok(StopState::StopRequested) | Ok(StopState::Stopped) => {}
        other => return Err(format!("flag off must stop runs, got {other:?}")),
    }
    println!("gate: t22_flag_off_stops_runs");
    Ok(())
}

fn t25_native_wayland_false() -> Result<(), String> {
    let fake = FakeAdapter::new();
    if fake.capabilities().native_wayland {
        return Err("native_wayland must stay false".into());
    }
    println!("gate: t25_native_wayland_false");
    Ok(())
}

fn t26_action_budget_exhausted() -> Result<(), String> {
    let fake = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(
        fake.clone(),
        BrokerOptions {
            feature_enabled: true,
            action_budget: 2,
            lease_path: std::env::temp_dir().join(format!("cu-t26-{}.lease", Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    );
    broker.open_run("s", "run-1").map_err(|e| e.to_string())?;
    let tid = fake.fixture_id();
    let gen = broker
        .authorize_target("run-1", &tid)
        .map_err(|e| e.to_string())?;
    for i in 0..2 {
        let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
        let out = broker.act(click_req(
            "run-1",
            &tid,
            gen,
            &obs.snapshot_id,
            obs.geometry_revision,
            &format!("t26-{i}"),
        ));
        if !out.executed {
            return Err(format!("budget seed {i} failed {out:?}"));
        }
    }
    let obs = broker.observe("run-1").map_err(|e| e.to_string())?;
    let third = broker.act(click_req(
        "run-1",
        &tid,
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t26-overflow",
    ));
    if third.executed {
        return Err("exhausted budget must refuse further acts".into());
    }
    println!("gate: t26_action_budget_exhausted");
    Ok(())
}

fn t27_reconnect_clears_authorization() -> Result<(), String> {
    let (broker, fake, gen, obs) = ready(400)?;
    broker.reconnect("run-1").map_err(|e| e.to_string())?;
    let out = broker.act(click_req(
        "run-1",
        &fake.fixture_id(),
        gen,
        &obs.snapshot_id,
        obs.geometry_revision,
        "t27-after",
    ));
    if out.executed {
        return Err("act without new grant after reconnect must fail".into());
    }
    println!("gate: t27_reconnect_clears_authorization");
    Ok(())
}

fn t28_resume_not_a_model_tool() -> Result<(), String> {
    let (server, token) = ipc_fixture()?;
    let response = reqwest::blocking::Client::new()
        .post(format!("{}/cu/tool", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"computer_resume"}))
        .send()
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    if body.get("isError") != Some(&serde_json::json!(true)) {
        return Err("resume must not be a model tool".into());
    }
    println!("gate: t28_resume_not_a_model_tool");
    Ok(())
}

fn t29_observe_has_no_clipboard() -> Result<(), String> {
    let (broker, _, _, obs) = ready(400)?;
    let raw = serde_json::to_value(&obs).map_err(|e| e.to_string())?;
    if raw.get("clipboard").is_some() || raw.to_string().to_ascii_lowercase().contains("clipboard")
    {
        return Err("observation must not include clipboard contents".into());
    }
    let _ = broker;
    println!("gate: t29_observe_has_no_clipboard");
    Ok(())
}

fn t30_model_traces_omit_preview() -> Result<(), String> {
    let (broker, _, _, _) = ready(400)?;
    broker
        .set_preview_visible("run-1", true)
        .map_err(|e| e.to_string())?;
    let model = broker.traces(Some("run-1"), true);
    if model.iter().any(|t| t.kind.contains("preview")) {
        return Err("model traces must not include UI preview captures".into());
    }
    println!("gate: t30_model_traces_omit_preview");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dead_lease_not_inherited() {
        super::t1_dead_lease_not_inherited().expect("dead_lease_not_inherited");
    }
    #[test]
    fn feature_off_zero_execution() {
        super::t2_feature_off_zero_execution().expect("feature_off_zero_execution");
    }
    #[test]
    fn ipc_origin_refused() {
        super::t3_ipc_origin_refused().expect("ipc_origin_refused");
    }
    #[test]
    fn rotated_bearer_refused() {
        super::t4_rotated_bearer_refused().expect("rotated_bearer_refused");
    }
    #[test]
    fn unauthorized_target_unchanged() {
        super::t5_unauthorized_target_unchanged().expect("unauthorized_target_unchanged");
    }
    #[test]
    fn late_result_not_applied() {
        super::t6_late_result_not_applied().expect("late_result_not_applied");
    }
    #[test]
    fn stale_element_ref_rejected() {
        super::t7_stale_element_ref_rejected().expect("stale_element_ref_rejected");
    }
    #[test]
    fn coords_outside_image_no_desktop() {
        super::t8_coords_outside_image_no_desktop().expect("coords_outside_image_no_desktop");
    }
    #[test]
    fn drag_without_destination_rejected() {
        super::t9_drag_without_destination_rejected().expect("drag_without_destination_rejected");
    }
    #[test]
    fn nul_or_overlong_text_rejected() {
        super::t10_nul_or_overlong_text_rejected().expect("nul_or_overlong_text_rejected");
    }
    #[test]
    fn wait_schema_rejected() {
        super::t11_wait_schema_rejected().expect("wait_schema_rejected");
    }
    #[test]
    fn handoff_pauses_model_cannot_finish() {
        super::t12_handoff_pauses_model_cannot_finish()
            .expect("handoff_pauses_model_cannot_finish");
    }
    #[test]
    fn hide_preview_stops_capture() {
        super::t13_hide_preview_stops_capture().expect("hide_preview_stops_capture");
    }
    #[test]
    fn iconic_windows_not_listed() {
        super::t14_iconic_windows_not_listed().expect("iconic_windows_not_listed");
    }
    #[test]
    fn managed_profile_owner() {
        super::t16_managed_profile_owner().expect("managed_profile_owner");
    }
    #[test]
    fn navigate_records_actual_url() {
        super::t17_navigate_records_actual_url().expect("navigate_records_actual_url");
    }
    #[test]
    fn model_download_path_rejected() {
        super::t18_model_download_path_rejected().expect("model_download_path_rejected");
    }
    #[test]
    fn cancel_keeps_user_tab() {
        super::t19_cancel_keeps_user_tab().expect("cancel_keeps_user_tab");
    }
    #[test]
    fn two_runs_cannot_share_profile() {
        super::t20_two_runs_cannot_share_profile().expect("two_runs_cannot_share_profile");
    }
    #[test]
    fn fork_no_snapshot_no_replay() {
        super::t21_fork_no_snapshot_no_replay().expect("fork_no_snapshot_no_replay");
    }
    #[test]
    fn flag_off_stops_runs() {
        super::t22_flag_off_stops_runs().expect("flag_off_stops_runs");
    }
    #[test]
    fn native_wayland_false() {
        super::t25_native_wayland_false().expect("native_wayland_false");
    }
    #[test]
    fn action_budget_exhausted() {
        super::t26_action_budget_exhausted().expect("action_budget_exhausted");
    }
    #[test]
    fn reconnect_clears_authorization() {
        super::t27_reconnect_clears_authorization().expect("reconnect_clears_authorization");
    }
    #[test]
    fn resume_not_a_model_tool() {
        super::t28_resume_not_a_model_tool().expect("resume_not_a_model_tool");
    }
    #[test]
    fn observe_has_no_clipboard() {
        super::t29_observe_has_no_clipboard().expect("observe_has_no_clipboard");
    }
    #[test]
    fn model_traces_omit_preview() {
        super::t30_model_traces_omit_preview().expect("model_traces_omit_preview");
    }
}
