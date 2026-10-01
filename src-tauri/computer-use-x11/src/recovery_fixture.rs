//! Recovery is an exact reply boundary, never a healthy-screen/timer reset.
use super::*;
use grok_computer_use_core::protocol::OutcomeKind;

const SEED: &str = "中文第二波🙂测试";

fn prepare(
    adapter: &LinuxAdapter,
    fixture: &mut Fixture,
    target: &str,
    run: &str,
    action: ActionKind,
) -> Result<DispatchRequest, String> {
    fixture.command("enable")?;
    let observed = adapter.observe_for_run(run, target)?;
    adapter.act(&request(
        &observed,
        "CU editable field",
        ActionKind::SetValue,
        json!({"text":SEED}),
    )?)?;
    fixture.command(if action == ActionKind::TypeText {
        "caret-middle"
    } else {
        "select-middle"
    })?;
    request(
        &observed,
        "CU editable field",
        action,
        json!({"text":"恢复🙂"}),
    )
}

fn wait_idle(adapter: &LinuxAdapter, run: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !adapter.is_idle(run) {
        if Instant::now() >= deadline {
            return Err("native reply never recovered its owner".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

pub(super) fn run(
    adapter: &Arc<LinuxAdapter>,
    fixture: &mut Fixture,
    target: &str,
) -> Result<(), String> {
    for (name, action, boundary) in [
        ("insert", ActionKind::TypeText, "insert"),
        ("set-value", ActionKind::SetValue, "delete"),
    ] {
        let req = prepare(
            adapter,
            fixture,
            target,
            &format!("owned-recovery-{name}"),
            action,
        )?;
        let before = fixture.command(&format!("after-{boundary}-stop"))?;
        let worker_adapter = adapter.clone();
        let worker_req = req.clone();
        let worker = std::thread::spawn(move || worker_adapter.act(&worker_req));
        if fixture.read()?[format!("{boundary}Boundary")] != true {
            return Err("no actual native recovery boundary".into());
        }
        // Wait for the real caller timeout while the native callback is still
        // held. Joining the caller does NOT imply the toolkit has returned.
        if worker
            .join()
            .map_err(|_| "recovery worker panicked")?
            .is_ok()
        {
            return Err("held native RPC was claimed successful".into());
        }
        let start = Instant::now();
        adapter.abort(&req.run_id, req.generation + 1)?;
        if adapter.is_idle(&req.run_id) || start.elapsed() > Duration::from_millis(200) {
            return Err("Stop/status blocked or falsely released pending native input".into());
        }
        if !adapter.capabilities().type_text.semantic
            || !adapter.target_alive(target)
            || adapter.is_idle(&req.run_id)
            || adapter.act(&req).is_ok()
        {
            return Err("healthy connection, Stop or repeat request bypassed quarantine".into());
        }
        if fixture.command(&format!("resume-{boundary}"))?[format!("{boundary}Resumed")] != true {
            return Err("held toolkit input was not resumed".into());
        }
        let after = fixture.state()?;
        let expected = if action == ActionKind::TypeText {
            "中文恢复🙂第二波🙂测试"
        } else {
            "恢复🙂"
        };
        if after["text"] != expected
            || after["insertEvents"].as_u64() != before["insertEvents"].as_u64().map(|n| n + 1)
        {
            return Err("late native effect was replayed or differs from its one dispatch".into());
        }
        wait_idle(adapter, &req.run_id)?;
        let mut stale = req.clone();
        stale.cancellation = Default::default();
        stale.generation += 1;
        if adapter.act(&stale).is_ok() || fixture.state()? != after {
            return Err("recovery transferred old authority to a new token/generation".into());
        }
        let fresh = adapter.observe_for_run(&req.run_id, target)?;
        let mut next = request(
            &fresh,
            "CU editable field",
            ActionKind::TypeText,
            json!({"text":"✓"}),
        )?;
        next.generation = stale.generation;
        let result = adapter.act(&next)?;
        // SetTextContents does not promise an end caret. Honor the actual GTK
        // caret instead of moving it or weakening the exact readback check.
        if after["selection"] != json!([]) {
            return Err("unexpected post-completion selection".into());
        }
        let caret = after["caret"].as_u64().ok_or("missing native caret")? as usize;
        let next_text = format!(
            "{}✓{}",
            expected.chars().take(caret).collect::<String>(),
            expected.chars().skip(caret).collect::<String>()
        );
        let actual = fixture.state()?;
        if result.outcome != Some(OutcomeKind::Verified) || actual["text"] != next_text {
            return Err(format!(
                "fresh native input differs: {result:?}; actual={actual}; expected={next_text}"
            ));
        }
        println!("INFO native {name} recovered caret={caret}; new input respects GTK position");
        println!("PASS native {name} timeout and Stop retain occupancy until exact reply then permit only freshly observed input");
    }
    Ok(())
}

pub(super) fn broker_acceptance(
    broker: &grok_computer_use_core::broker::ComputerUseBroker,
    adapter: &LinuxAdapter,
    fixture: &mut Fixture,
) -> Result<(), String> {
    use grok_computer_use_core::protocol::{ActionRequest, PROTOCOL_VERSION};
    fixture.command("enable")?;
    let obs = broker.observe("broker-atspi").map_err(|e| e.to_string())?;
    let node = obs
        .nodes
        .iter()
        .find(|n| n.name == "CU editable field")
        .ok_or("missing Broker edit ref")?;
    let mut req = ActionRequest {
        version: PROTOCOL_VERSION,
        run_id: obs.run_id,
        action_id: "broker-recovery-seed".into(),
        target_id: obs.target_id,
        target_generation: obs.target_generation,
        snapshot_id: obs.snapshot_id,
        geometry_revision: obs.geometry_revision,
        action: ActionKind::SetValue,
        target: ActionTarget::Element {
            element_ref: node.node_ref.clone(),
        },
        parameters: json!({"text":SEED}),
    };
    if broker.act(req.clone()).kind != OutcomeKind::Verified {
        return Err("Broker recovery seed failed".into());
    }
    fixture.command("select-middle")?;
    let before = fixture.command("after-delete-lost-reply")?;
    req.action_id = "broker-recovery-unknown".into();
    req.action = ActionKind::TypeText;
    req.parameters = json!({"text":"NEVER INSERTED"});
    let unknown = broker.act(req.clone());
    if unknown.kind != OutcomeKind::Unknown {
        return Err(format!("Broker lost unknown native effect: {unknown:?}"));
    }
    let after = fixture.state()?;
    if after["text"] != "中文测试" || after["insertEvents"] != before["insertEvents"] {
        return Err("unknown Broker input retried/continued".into());
    }
    wait_idle(adapter, "broker-atspi")?;
    if broker.act(req.clone()).kind != OutcomeKind::Unknown || fixture.state()? != after {
        return Err("late reply upgraded or replayed cached Unknown".into());
    }
    let obs = broker.observe("broker-atspi").map_err(|e| e.to_string())?;
    req.target = ActionTarget::Element {
        element_ref: obs
            .nodes
            .iter()
            .find(|n| n.name == "CU editable field")
            .ok_or("fresh Broker ref absent")?
            .node_ref
            .clone(),
    };
    req.snapshot_id = obs.snapshot_id;
    req.geometry_revision = obs.geometry_revision;
    req.action_id = "broker-recovery-new-independent-input".into();
    req.action = ActionKind::SetValue;
    req.parameters = json!({"text":"BROKER RECOVERED 中文🙂"});
    if broker.act(req).kind != OutcomeKind::Verified
        || fixture.state()?["text"] != "BROKER RECOVERED 中文🙂"
    {
        return Err("fresh Broker action failed after exact native recovery".into());
    }
    println!("PASS Broker late completion preserves cached Unknown without replay and requires fresh observation for independent input");
    Ok(())
}

/// Last gate: kill only the owned GTK process while its input RPC is blocked.
/// The bus-generated NoReply/vanished target must NOT count as a native reply.
pub(super) fn transport_loss(
    adapter: &Arc<LinuxAdapter>,
    fixture: &mut Fixture,
    target: &str,
) -> Result<(), String> {
    let req = prepare(
        adapter,
        fixture,
        target,
        "owned-recovery-transport-loss",
        ActionKind::SetValue,
    )?;
    fixture.command("after-delete-stop")?;
    let a = adapter.clone();
    let r = req.clone();
    let worker = std::thread::spawn(move || a.act(&r));
    if fixture.read()?["deleteBoundary"] != true {
        return Err("transport loss lacks native in-flight proof".into());
    }
    if worker
        .join()
        .map_err(|_| "transport worker panicked")?
        .is_ok()
    {
        return Err("blocked native RPC was successful".into());
    }
    fixture.child.kill().map_err(|e| e.to_string())?;
    fixture.child.wait().map_err(|e| e.to_string())?;
    adapter.abort(&req.run_id, req.generation + 1)?;
    for _ in 0..25 {
        if adapter.is_idle(&req.run_id) {
            return Err("provider death/bus error falsely confirmed native completion".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if adapter.act(&req).is_ok() {
        return Err("transport loss permitted native replay".into());
    }
    println!(
        "PASS provider death and bus NoReply do not recover unknown native input or permit replay"
    );
    Ok(())
}
