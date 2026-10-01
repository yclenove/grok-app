//! Selected-range acceptance against the actual GTK process, not engine mocks.
use super::*;
use grok_computer_use_core::adapter::AdapterActResult;
use grok_computer_use_core::protocol::OutcomeKind;

const SEED: &str = "中文第二波🙂测试";

fn prepare(
    adapter: &LinuxAdapter,
    fixture: &mut Fixture,
    target: &str,
    run: &str,
) -> Result<DispatchRequest, String> {
    fixture.command("enable")?;
    let obs = adapter.observe_for_run(run, target)?;
    let mut req = request(
        &obs,
        "CU editable field",
        ActionKind::SetValue,
        json!({"value":SEED}),
    )?;
    if !adapter.act(&req)?.postcondition_ok || fixture.state()?["text"] != SEED {
        return Err("selection seed did not reach the real native field".into());
    }
    fixture.command("select-middle")?;
    req.action = ActionKind::TypeText;
    req.parameters = json!({"text":"替换🦀"});
    Ok(req)
}

fn partial(result: AdapterActResult) -> Result<(), String> {
    if !result.applied
        || result.postcondition_ok
        || result.verifiable
        || result.outcome != Some(OutcomeKind::Applied)
        || !result.detail.contains("partial text edit")
    {
        return Err(format!("partial native edit was misclassified: {result:?}"));
    }
    Ok(())
}

pub(super) fn run(
    adapter: &Arc<LinuxAdapter>,
    fixture: &mut Fixture,
    target: &str,
) -> Result<(), String> {
    for (command, text, expected, caret) in [
        ("select-reverse", "替换🦀", "中文替换🦀测试", 5),
        ("select-all", "全新🙂", "全新🙂", 3),
        (
            "caret-middle",
            "a\u{301}🦀",
            "中文a\u{301}🦀第二波🙂测试",
            5,
        ),
    ] {
        let mut req = prepare(adapter, fixture, target, "owned-selection-positive")?;
        fixture.command(command)?;
        req.parameters = json!({"text":text});
        let before = fixture.state()?;
        let result = adapter.act(&req)?;
        let after = fixture.state()?;
        let deleted = if command == "caret-middle" { 0 } else { 1 };
        if !result.postcondition_ok
            || after["text"] != expected
            || after["caret"] != caret
            || after["selection"] != json!([])
            || after["deleteEvents"].as_u64()
                != before["deleteEvents"].as_u64().map(|n| n + deleted)
            || after["insertEvents"].as_u64() != before["insertEvents"].as_u64().map(|n| n + 1)
        {
            return Err(format!(
                "native {command} text/caret/event oracle failed: {after}"
            ));
        }
        println!("PASS native {command} preserves code-point offsets and exact edit event counts");
    }
    let mut req = prepare(adapter, fixture, target, "owned-selection-empty")?;
    req.parameters = json!({"text":""});
    let before = fixture.state()?;
    let result = adapter.act(&req)?;
    if result.applied || !result.postcondition_ok || fixture.state()? != before {
        return Err("empty TypeText changed selected text, caret or event counters".into());
    }
    println!("PASS empty native TypeText preserves selected text and emits zero edit events");

    for mode in ["tamper", "disable"] {
        let req = prepare(adapter, fixture, target, &format!("owned-selection-{mode}"))?;
        let before = fixture.command(&format!("after-delete-{mode}"))?;
        partial(adapter.act(&req)?)?;
        let after = fixture.state()?;
        if mode == "tamper" {
            if after["text"] != "USER EDIT SURVIVES"
                || after["insertEvents"].as_u64() != before["insertEvents"].as_u64().map(|n| n + 1)
            {
                return Err(
                    "adapter overwrote/replayed over the native intervening user edit".into(),
                );
            }
        } else if after["text"] != "中文测试" || after["insertEvents"] != before["insertEvents"]
        {
            return Err("adapter inserted after the native control disabled itself".into());
        }
        if !adapter.is_idle(&req.run_id) {
            return Err("acknowledged partial edit retained busy ownership".into());
        }
        println!(
            "PASS native {mode} after deletion stops further input and reports partial effects"
        );
    }

    let req = prepare(adapter, fixture, target, "owned-selection-stop")?;
    let before = fixture.command("after-delete-stop")?;
    let worker_adapter = adapter.clone();
    let worker_req = req.clone();
    let worker = std::thread::spawn(move || worker_adapter.act(&worker_req));
    let boundary = fixture.read()?;
    if boundary["deleteBoundary"] != true || adapter.is_idle(&req.run_id) {
        return Err("owned Stop did not reach the actual in-flight DeleteText boundary".into());
    }
    let queried = Instant::now();
    let cold_capabilities = adapter.capabilities().type_text.semantic;
    let query_time = queried.elapsed();
    let started = Instant::now();
    adapter.abort(&req.run_id, req.generation + 1)?;
    if started.elapsed() > Duration::from_millis(500) {
        return Err("Stop waited on the toolkit's in-flight call".into());
    }
    if fixture.command("resume-delete")?["deleteResumed"] != true {
        return Err("delete boundary did not resume".into());
    }
    partial(
        worker
            .join()
            .map_err(|_| "native typing worker panicked")??,
    )?;
    if !cold_capabilities || query_time > Duration::from_millis(200) {
        return Err(
            "first capability query lost semantic support or blocked during native input".into(),
        );
    }
    println!("PASS first capability query during native input uses observed bus availability without blocking");
    let after = fixture.state()?;
    if after["text"] != "中文测试"
        || after["insertEvents"] != before["insertEvents"]
        || !adapter.is_idle(&req.run_id)
    {
        return Err(
            "cancelStop continued insertion or failed to release acknowledged input".into(),
        );
    }
    let mut retired = req.clone();
    retired.cancellation = Default::default();
    if adapter.act(&retired).is_ok() || fixture.state()? != after {
        return Err("Stop retired reference revived".into());
    }
    println!("PASS native in-flight DeleteText Stop prevents insertion and retires old authority");

    // The native reply is deliberately late. Only that exact reply may release
    // occupancy, never the independent fixture readback or elapsed time.
    let isolated = LinuxAdapter::new();
    let isolated_target = isolated
        .list_targets()?
        .into_iter()
        .find(|t| t.title == "CU owned accessibility fixture")
        .ok_or("isolated adapter did not discover its own target lifetime")?
        .target_id;
    let req = prepare(
        &isolated,
        fixture,
        &isolated_target,
        "owned-selection-lost-reply",
    )?;
    let before = fixture.command("after-delete-lost-reply")?;
    if isolated.act(&req).is_ok() || isolated.is_idle(&req.run_id) {
        return Err("lost native reply was accepted or released input ownership".into());
    }
    let after = fixture.state()?; // Toolkit has actually completed before reading the oracle.
    if after["text"] != "中文测试" || after["insertEvents"] != before["insertEvents"] {
        return Err("lost native reply caused retry or insertion".into());
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while !isolated.is_idle(&req.run_id) {
        if Instant::now() >= deadline {
            return Err("exact late native reply did not recover occupancy".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if isolated.act(&req).is_ok() || fixture.state()? != after {
        return Err("late reply revived stale authority or replayed input".into());
    }
    println!(
        "PASS late native DeleteText reply recovers exact occupancy and retires stale authority without replay"
    );
    Ok(())
}

pub(super) fn broker_acceptance(
    broker: &grok_computer_use_core::broker::ComputerUseBroker,
    fixture: &mut Fixture,
    _target: &str,
) -> Result<(), String> {
    use grok_computer_use_core::protocol::{ActionRequest, PROTOCOL_VERSION};
    let observed = broker.observe("broker-atspi").map_err(|e| e.to_string())?;
    let ref_id = observed
        .nodes
        .iter()
        .find(|n| n.name == "CU editable field")
        .ok_or("Broker lost editable field")?
        .node_ref
        .clone();
    let mut req = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "broker-selection-seed".into(),
        run_id: "broker-atspi".into(),
        target_id: observed.target_id,
        target_generation: observed.target_generation,
        snapshot_id: observed.snapshot_id,
        geometry_revision: observed.geometry_revision,
        action: ActionKind::SetValue,
        target: ActionTarget::Element {
            element_ref: ref_id,
        },
        parameters: json!({"text":SEED}),
    };
    let seeded = broker.act(req.clone());
    if seeded.kind != OutcomeKind::Verified {
        return Err(format!("Broker text seed failed: {seeded:?}"));
    }
    fixture.command("select-middle")?;
    fixture.command("after-delete-disable")?;
    req.action = ActionKind::TypeText;
    req.action_id = "broker-selection-partial".into();
    req.parameters = json!({"text":"NOT INSERTED"});
    let result = broker.act(req.clone());
    let after = fixture.state()?;
    if result.kind != OutcomeKind::Applied || !result.executed || after["text"] != "中文测试" {
        return Err(format!(
            "Broker partial native edit lost its applied status: {result:?}"
        ));
    }
    let replay = broker.act(req);
    if replay.kind != OutcomeKind::Applied || fixture.state()? != after {
        return Err("Broker replayed partial edit".into());
    }
    println!("PASS production Broker preserves partial native edit outcome and deduplicates without replay");
    Ok(())
}
