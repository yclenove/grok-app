//! Real adapter -> owned Cocoa process -> independent application postconditions.
//! No doubles are linked by cu-macos-native on macOS. Other OSes return not_run.
use super::macos_native_pipe::Fixture;
use super::macos_native_protocol::{key_pair, State};
use base64::Engine;
use grok_computer_use_core::adapter::{
    ActionScope, CaptureOptions, ComputerUseAdapter, DispatchRequest, TargetInfo,
};
use grok_computer_use_core::protocol::{ActionKind, ActionTarget, Observation, ObservationNode};
use grok_computer_use_core::quartz_frame::WindowInstance;
use serde::Serialize;
use serde_json::json;
use std::io::{Cursor, Write};
use std::path::Path;

pub const GATES: &[&str] = &[
    "owned-process-window",
    "screenshot-and-protected-ax",
    "semantic-unicode-append",
    "coordinate-unicode-append",
    "coordinate-left-pair",
    "semantic-right-pair",
    "preview-rejected",
    "stale-snapshot-rejected",
    "imageless-coordinate-rejected",
    "unknown-ref-rejected",
    "cancelled-action-rejected",
    "moved-window-rejected",
    "fresh-observation-after-move",
    "semantic-press-once",
    "coordinate-press-once",
    "pointer-double-click",
    "pointer-right-click",
    "pointer-middle-click",
    "pointer-scroll-down",
    "pointer-scroll-up",
    "pointer-zero-scroll",
    "pointer-drag-forward",
    "pointer-drag-back",
    "retained-name-wait",
    "retained-name-wait-timeout",
    "retained-name-wait-stale-reference",
    "abort-retires-observation",
    "closed-window-rejected",
    "owned-cleanup",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub status: String,
    pub host: &'static str,
    pub architecture: &'static str,
    pub required: &'static [&'static str],
    pub passed: Vec<&'static str>,
    pub error: Option<String>,
}

impl Report {
    pub fn not_run(reason: &str) -> Self {
        Self {
            status: "not_run".into(),
            host: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            required: GATES,
            passed: Vec::new(),
            error: Some(reason.into()),
        }
    }
    pub(super) fn pass(
        &mut self,
        gate: &'static str,
        state: &State,
        output: &Path,
    ) -> Result<(), String> {
        if GATES.get(self.passed.len()) != Some(&gate) {
            return Err("native gate order mismatch".into());
        }
        write_new(
            &output.join(format!("{:02}-{gate}.json", self.passed.len() + 1)),
            &serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?,
        )?;
        self.passed.push(gate);
        eprintln!("native gate passed: {gate}");
        Ok(())
    }
    pub fn finish(&mut self, result: Result<(), String>) {
        self.status = "failed".into();
        self.error = match result {
            Err(error) => Some(error),
            Ok(()) if self.passed != GATES => Some("native acceptance matrix incomplete".into()),
            Ok(()) => {
                self.status = "passed".into();
                None
            }
        };
    }
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("create fresh evidence {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}

pub fn owned_target(targets: &[TargetInfo], state: &State) -> Result<String, String> {
    let matches: Vec<_> = targets
        .iter()
        .filter(|t| t.pid == Some(state.pid) && t.title == state.title)
        .collect();
    let [target] = matches.as_slice() else {
        return Err("exact owned window not uniquely listed".into());
    };
    let instance = WindowInstance::parse(&target.target_id)?;
    if instance.window.pid != state.pid
        || instance.window.wid != state.window_id
        || target.backend != "macos"
        || target.kind != "window"
        || target.coordinate_space != "image-pixels"
        || target.lifecycle_stamp != instance.lifecycle_stamp()
    {
        return Err("listed target does not bind the owned native process/window".into());
    }
    Ok(target.target_id.clone())
}

fn node<'a>(obs: &'a Observation, name: &str, action: &str) -> Result<&'a ObservationNode, String> {
    let matches: Vec<_> = obs
        .nodes
        .iter()
        .filter(|n| n.name == name && n.actions.iter().any(|a| a == action))
        .collect();
    match matches.as_slice() {
        [node] if !node.node_ref.is_empty() && !node.truncated => Ok(node),
        _ => Err(format!(
            "owned observation has no unique {name}/{action} node"
        )),
    }
}

pub fn center(node: &ObservationNode, obs: &Observation) -> Result<ActionTarget, String> {
    let (Some(x), Some(y), Some(w), Some(h)) = (node.x, node.y, node.width, node.height) else {
        return Err("owned control has no image bounds".into());
    };
    if ![x, y, w, h, x + w, y + h].iter().all(|v| v.is_finite())
        || x < 0.0
        || y < 0.0
        || w <= 0.0
        || h <= 0.0
        || x + w > f64::from(obs.image.width)
        || y + h > f64::from(obs.image.height)
    {
        return Err("owned control lies outside its screenshot".into());
    }
    Ok(ActionTarget::Coord {
        x: x + w / 2.0,
        y: y + h / 2.0,
    })
}

pub(super) fn request(
    obs: &Observation,
    action: ActionKind,
    target: ActionTarget,
    parameters: serde_json::Value,
) -> DispatchRequest {
    DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: obs.run_id.clone(),
        action_id: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        target_id: obs.target_id.clone(),
        target_generation: 1,
        snapshot_id: obs.snapshot_id.clone(),
        geometry_revision: obs.geometry_revision,
        action,
        target,
        parameters,
        scope: ActionScope::Directed,
    }
}

fn text_request(
    obs: &Observation,
    name: &str,
    coordinate: bool,
    text: &str,
) -> Result<DispatchRequest, String> {
    let node = node(obs, name, "type_text")?;
    let target = if coordinate {
        center(node, obs)?
    } else {
        ActionTarget::Element {
            element_ref: node.node_ref.clone(),
        }
    };
    Ok(request(
        obs,
        ActionKind::TypeText,
        target,
        json!({"text":text}),
    ))
}

pub(super) fn applied(
    adapter: &dyn ComputerUseAdapter,
    req: &DispatchRequest,
) -> Result<(), String> {
    let result = adapter.act(req)?;
    // Physical effect is verified ONLY by the owned Cocoa process, not this return flag.
    if !result.applied || result.verifiable || result.postcondition_ok {
        return Err(format!(
            "unexpected adapter dispatch classification: {result:?}"
        ));
    }
    Ok(())
}

fn rejected(
    adapter: &dyn ComputerUseAdapter,
    fixture: &mut Fixture,
    req: &DispatchRequest,
) -> Result<State, String> {
    let before = fixture.request("state")?;
    if adapter.act(req).is_ok() {
        return Err("invalid authority unexpectedly dispatched".into());
    }
    fixture.quiet(&before)
}

pub fn verified_read_only_wait(
    result: &grok_computer_use_core::adapter::AdapterActResult,
) -> Result<(), String> {
    if result.applied || !result.verifiable || !result.postcondition_ok || result.outcome.is_some()
    {
        return Err(format!(
            "unexpected read-only wait classification: {result:?}"
        ));
    }
    Ok(())
}

pub(super) fn screenshot(obs: &Observation, output: &Path) -> Result<(), String> {
    if obs.coordinate_space != "image-pixels" || obs.truncated || obs.snapshot_id.is_empty() {
        return Err("owned screenshot observation incomplete".into());
    }
    let encoded = obs
        .image
        .png_base64
        .as_ref()
        .ok_or("native screenshot absent")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| e.to_string())?;
    let mut reader = png::Decoder::new(Cursor::new(&bytes))
        .read_info()
        .map_err(|e| e.to_string())?;
    let size = reader
        .output_buffer_size()
        .filter(|s| *s <= 64 * 1024 * 1024)
        .ok_or("PNG buffer limit")?;
    let frame = reader
        .next_frame(&mut vec![0; size])
        .map_err(|e| e.to_string())?;
    if frame.width != obs.image.width
        || frame.height != obs.image.height
        || frame.width == 0
        || frame.height == 0
    {
        return Err("native PNG and observation dimensions differ".into());
    }
    write_new(&output.join("owned-window.png"), &bytes)?;
    write_new(
        &output.join("initial-observation.json"),
        &serde_json::to_vec_pretty(obs).map_err(|e| e.to_string())?,
    )
}

fn gates(
    adapter: &dyn ComputerUseAdapter,
    fixture: &mut Fixture,
    output: &Path,
    report: &mut Report,
) -> Result<(), String> {
    fixture.request("focus")?;
    let initial = fixture.wait(|s| s.active && s.focused)?;
    if initial.text != "原有😀text"
        || initial.selection != [0, 0]
        || !initial.keys.is_empty()
        || initial.clicks != 0
        || !initial.pointer.fresh()
    {
        return Err("owned fixture baseline is not fresh".into());
    }
    let target = owned_target(&adapter.list_targets()?, &initial)?;
    report.pass(GATES[0], &initial, output)?;
    let run = format!("native-{}", fixture.nonce);
    let name = format!("Owned input {}", fixture.nonce);
    let press = format!("Owned press {}", fixture.nonce);
    let observe = || adapter.observe_for_run(&run, &target);
    let mut obs = observe()?;
    if obs.run_id != run || obs.target_id != target {
        return Err("observation does not belong to owned run/window".into());
    }
    screenshot(&obs, output)?;
    let serialized = serde_json::to_string(&obs).map_err(|e| e.to_string())?;
    if serialized.contains("fixture-only-never-observe-")
        || obs
            .nodes
            .iter()
            .any(|n| n.role == "AXSecureTextField" || n.name.starts_with("Owned protected "))
    {
        return Err("protected fixture text/control leaked into AX observation".into());
    }
    node(&obs, &name, "type_text")?;
    report.pass(GATES[1], &initial, output)?;
    let mut expected = initial.text.clone();
    for (coordinate, suffix, gate) in [(false, "语义😀", GATES[2]), (true, "坐标😀z", GATES[3])]
    {
        obs = observe()?;
        applied(adapter, &text_request(&obs, &name, coordinate, suffix)?)?;
        expected.push_str(suffix);
        let count = expected.encode_utf16().count();
        let after = fixture.wait(|s| s.text == expected && s.selection == [count, 0])?;
        if !after.keys.is_empty() || after.clicks != 0 {
            return Err("text input used key/click fallback".into());
        }
        let after = fixture.quiet(&after)?;
        report.pass(gate, &after, output)?;
    }
    for (coordinate, key, code, offset, gate) in [
        (true, "left", 123, 1, GATES[4]),
        (false, "right", 124, 0, GATES[5]),
    ] {
        let before = fixture.request("state")?;
        obs = observe()?;
        let n = node(&obs, &name, "key")?;
        let destination = if coordinate {
            center(n, &obs)?
        } else {
            ActionTarget::Element {
                element_ref: n.node_ref.clone(),
            }
        };
        applied(
            adapter,
            &request(&obs, ActionKind::Key, destination, json!({"key":key})),
        )?;
        let count = expected.encode_utf16().count() - offset;
        let after = fixture.wait(|s| {
            s.text == expected
                && s.selection == [count, 0]
                && key_pair(s, &before.keys, code).is_ok()
        })?;
        if after.clicks != before.clicks {
            return Err("named key changed click count".into());
        }
        let after = fixture.quiet(&after)?;
        report.pass(gate, &after, output)?;
    }
    let preview = adapter.observe(&target)?;
    let after = rejected(
        adapter,
        fixture,
        &text_request(&preview, &name, true, "FORBIDDEN")?,
    )?;
    report.pass(GATES[6], &after, output)?;
    obs = observe()?;
    let stale = text_request(&obs, &name, true, "FORBIDDEN")?;
    observe()?;
    let after = rejected(adapter, fixture, &stale)?;
    report.pass(GATES[7], &after, output)?;
    obs = adapter.capture_for_run(&run, &target, CaptureOptions::model(false))?;
    let after = rejected(
        adapter,
        fixture,
        &text_request(&obs, &name, true, "FORBIDDEN")?,
    )?;
    report.pass(GATES[8], &after, output)?;
    obs = observe()?;
    let bad = request(
        &obs,
        ActionKind::TypeText,
        ActionTarget::Element {
            element_ref: "unobserved".into(),
        },
        json!({"text":"FORBIDDEN"}),
    );
    let after = rejected(adapter, fixture, &bad)?;
    report.pass(GATES[9], &after, output)?;
    let cancelled = text_request(&obs, &name, true, "FORBIDDEN")?;
    cancelled.cancellation.cancel();
    let after = rejected(adapter, fixture, &cancelled)?;
    report.pass(GATES[10], &after, output)?;
    let moved = text_request(&obs, &name, true, "FORBIDDEN")?;
    fixture.request("move")?;
    if adapter.current_geometry_revision_for(&target) == obs.geometry_revision {
        return Err("owned move did not change geometry revision".into());
    }
    let after = rejected(adapter, fixture, &moved)?;
    report.pass(GATES[11], &after, output)?;
    obs = observe()?;
    applied(adapter, &text_request(&obs, &name, true, "新观察")?)?;
    expected.push_str("新观察");
    let after = fixture
        .wait(|s| s.text == expected && s.selection == [expected.encode_utf16().count(), 0])?;
    let after = fixture.quiet(&after)?;
    report.pass(GATES[12], &after, output)?;
    for (coordinate, clicks, gate) in [(false, 1, GATES[13]), (true, 2, GATES[14])] {
        obs = observe()?;
        let n = node(&obs, &press, "click")?;
        let destination = if coordinate {
            center(n, &obs)?
        } else {
            ActionTarget::Element {
                element_ref: n.node_ref.clone(),
            }
        };
        applied(
            adapter,
            &request(&obs, ActionKind::Click, destination, json!({})),
        )?;
        let after = fixture.wait(|s| s.clicks == clicks && s.text == expected)?;
        let after = fixture.quiet(&after)?;
        report.pass(gate, &after, output)?;
    }
    super::macos_native_pointer_gates::execute(adapter, fixture, output, report, &run, &target)?;
    fixture.request("focus")?;
    fixture.wait(|s| s.active && s.focused)?;
    obs = observe()?;
    let wait = request(
        &obs,
        ActionKind::Wait,
        ActionTarget::Element {
            element_ref: node(&obs, &name, "wait")?.node_ref.clone(),
        },
        json!({"nameEquals":name,"timeoutMs":2000}),
    );
    let before = fixture.request("state")?;
    verified_read_only_wait(&adapter.act(&wait)?)?;
    let after = fixture.quiet(&before)?;
    report.pass("retained-name-wait", &after, output)?;
    let mut miss = wait.clone();
    miss.action_id = uuid::Uuid::new_v4().to_string();
    miss.parameters = json!({"nameEquals":"FIXTURE-ABSENT-WAIT-LABEL","timeoutMs":100});
    if !adapter
        .act(&miss)
        .is_err_and(|e| e.contains("wait timed out"))
    {
        return Err("native name wait did not reject the absent label at its deadline".into());
    }
    let after = fixture.quiet(&after)?;
    report.pass("retained-name-wait-timeout", &after, output)?;
    // A timeout is not a new observation: the original native identity remains
    // usable. Conversely a new model observation must retire these exact refs.
    verified_read_only_wait(&adapter.act(&wait)?)?;
    observe()?;
    let after = rejected(adapter, fixture, &wait)?;
    report.pass("retained-name-wait-stale-reference", &after, output)?;
    obs = observe()?;
    let retired = text_request(&obs, &name, true, "FORBIDDEN")?;
    adapter.abort(&run, 2)?;
    let after = rejected(adapter, fixture, &retired)?;
    report.pass("abort-retires-observation", &after, output)?;
    // Fresh generation/observation before close prevents "already aborted" from
    // accidentally proving the closed-window check.
    obs = observe()?;
    let mut closed = text_request(&obs, &name, true, "FORBIDDEN")?;
    closed.generation = 3;
    let final_state = fixture.request("quit")?;
    fixture.shutdown()?;
    if adapter.target_alive(&target) || adapter.act(&closed).is_ok() {
        return Err("closed owned window still accepts input".into());
    }
    report.pass("closed-window-rejected", &final_state, output)?;
    adapter.release_target_for_run(&run, &target);
    if !adapter.is_idle(&run) {
        return Err("adapter input occupancy remains unresolved".into());
    }
    report.pass("owned-cleanup", &final_state, output)
}

pub fn failure_with_cleanup(
    original: &str,
    evidence: Result<(), String>,
    cleanup: impl FnOnce() -> Result<(), String>,
) -> String {
    // A full disk or an existing evidence file must not strand the owned child.
    // Keep the original gate failure and both secondary failures independently.
    let cleanup = cleanup();
    let mut errors = vec![original.to_owned()];
    if let Err(error) = evidence {
        errors.push(format!("failure evidence: {error}"));
    }
    if let Err(error) = cleanup {
        errors.push(format!("cleanup: {error}"));
    }
    errors.join("; ")
}

pub fn execute(adapter: Option<&dyn ComputerUseAdapter>, path: &Path, output: &Path) -> Report {
    let Some(adapter) = adapter else {
        return Report::not_run("this binary has no native macOS backend on this OS");
    };
    if !adapter.input_available() {
        return Report::not_run("existing Screen Recording and Accessibility grants required; no permission prompt/grant attempted");
    }
    let mut report = Report::not_run("not started");
    let result = (|| {
        let mut fixture = Fixture::start(path)?;
        let result = gates(adapter, &mut fixture, output, &mut report);
        if let Err(error) = &result {
            let evidence = fixture.state.as_ref().map_or(Ok(()), |state| {
                serde_json::to_vec_pretty(state)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| write_new(&output.join("failure-state.json"), &bytes))
            });
            return Err(failure_with_cleanup(error, evidence, || {
                // Always stop the same owned process, even if saving evidence failed.
                let _ = fixture.request("quit");
                fixture.shutdown()
            }));
        }
        result
    })();
    report.finish(result);
    report
}
