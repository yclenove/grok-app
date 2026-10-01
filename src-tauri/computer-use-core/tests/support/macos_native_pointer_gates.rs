//! Actual adapter dispatch; all effect data comes back from the owned Cocoa app.
use super::macos_native_gates::{applied, center, request, screenshot, Report};
use super::macos_native_pipe::Fixture;
use super::macos_native_pointer as pointer;
use super::macos_native_protocol::State;
use grok_computer_use_core::adapter::ComputerUseAdapter;
use grok_computer_use_core::protocol::{ActionKind, ActionTarget, Observation};
use serde_json::json;
use std::path::Path;

fn coordinate(obs: &Observation, nonce: &str, point: [f64; 2]) -> Result<ActionTarget, String> {
    let name = format!("Owned pointer {nonce}");
    let found: Vec<_> = obs
        .nodes
        .iter()
        .filter(|n| n.name == name && n.role == "AXGroup" && !n.truncated)
        .collect();
    let [node] = found.as_slice() else {
        return Err("owned pointer view not uniquely observed".into());
    };
    let ActionTarget::Coord { x, y } = center(node, obs)? else {
        unreachable!()
    };
    if !point
        .iter()
        .all(|v| v.is_finite() && (0.0..1.0).contains(v))
    {
        return Err("pointer fixture coordinate out of bounds".into());
    }
    Ok(ActionTarget::Coord {
        x: x + (point[0] - 0.5) * node.width.ok_or("pointer width missing")?,
        y: y + (point[1] - 0.5) * node.height.ok_or("pointer height missing")?,
    })
}

fn other_controls_unchanged(before: &State, after: &State) -> Result<(), String> {
    if before.text != after.text
        || before.selection != after.selection
        || before.keys != after.keys
        || before.clicks != after.clicks
    {
        return Err("pointer input changed another owned control".into());
    }
    Ok(())
}

pub(super) fn execute(
    adapter: &dyn ComputerUseAdapter,
    fixture: &mut Fixture,
    output: &Path,
    report: &mut Report,
    run: &str,
    target: &str,
) -> Result<(), String> {
    let start = fixture.request("state")?;
    if !start.pointer.fresh() {
        return Err("pointer baseline was modified before its gate".into());
    }
    for (name, button, count, gate) in [
        ("left", 0, 2, "pointer-double-click"),
        ("right", 1, 1, "pointer-right-click"),
        ("middle", 2, 1, "pointer-middle-click"),
    ] {
        let before = fixture.request("state")?;
        let obs = adapter.observe_for_run(run, target)?;
        applied(
            adapter,
            &request(
                &obs,
                ActionKind::Click,
                coordinate(&obs, &fixture.nonce, [0.5, 0.5])?,
                json!({"button":name,"count":count}),
            ),
        )?;
        let after = fixture.wait(|s| {
            pointer::click(&before.pointer, &s.pointer, [0.5, 0.5], button, count).is_ok()
        })?;
        other_controls_unchanged(&before, &after)?;
        let after = fixture.quiet(&after)?;
        report.pass(gate, &after, output)?;
    }
    for (delta, gate) in [
        (120, "pointer-scroll-down"),
        (-120, "pointer-scroll-up"),
        (0, "pointer-zero-scroll"),
    ] {
        let before = fixture.request("state")?;
        let obs = adapter.observe_for_run(run, target)?;
        let req = request(
            &obs,
            ActionKind::Scroll,
            coordinate(&obs, &fixture.nonce, [0.5, 0.5])?,
            json!({"delta":delta}),
        );
        let after = if delta == 0 {
            let result = adapter.act(&req)?;
            if result.applied || result.verifiable || result.postcondition_ok {
                return Err("zero native scroll did not report a no-op".into());
            }
            fixture.quiet(&before)?
        } else {
            applied(adapter, &req)?;
            fixture
                .wait(|s| pointer::scroll(&before.pointer, &s.pointer, [0.5, 0.5], delta).is_ok())?
        };
        pointer::scroll(&before.pointer, &after.pointer, [0.5, 0.5], delta)?;
        other_controls_unchanged(&before, &after)?;
        let after = fixture.quiet(&after)?;
        let image = output.join(gate);
        std::fs::create_dir(&image).map_err(|e| e.to_string())?;
        screenshot(&adapter.observe_for_run(run, target)?, &image)?;
        report.pass(gate, &after, output)?;
    }
    for (from, to, gate) in [
        ([0.5, 0.5], [0.72, 0.72], "pointer-drag-forward"),
        ([0.72, 0.72], [0.5, 0.5], "pointer-drag-back"),
    ] {
        let before = fixture.request("state")?;
        let obs = adapter.observe_for_run(run, target)?;
        let ActionTarget::Coord { x, y } = coordinate(&obs, &fixture.nonce, to)? else {
            unreachable!()
        };
        applied(
            adapter,
            &request(
                &obs,
                ActionKind::Drag,
                coordinate(&obs, &fixture.nonce, from)?,
                json!({"toX":x,"toY":y}),
            ),
        )?;
        let after =
            fixture.wait(|s| pointer::drag(&before.pointer, &s.pointer, from, to).is_ok())?;
        other_controls_unchanged(&before, &after)?;
        let after = fixture.quiet(&after)?;
        let image = output.join(gate);
        std::fs::create_dir(&image).map_err(|e| e.to_string())?;
        screenshot(&adapter.observe_for_run(run, target)?, &image)?;
        report.pass(gate, &after, output)?;
    }
    Ok(())
}
