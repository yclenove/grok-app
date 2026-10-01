//! Real GTK + AT-SPI acceptance, independently read back by the owned GTK process.
use crate::{ComputerUseAdapter, DispatchRequest, LinuxAdapter};
use grok_computer_use_core::adapter::ActionScope;
use grok_computer_use_core::protocol::{ActionKind, ActionTarget, Observation};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{
    mpsc::{self, Receiver},
    Arc,
};
use std::time::{Duration, Instant};

#[path = "recovery_fixture.rs"]
mod recovery;
#[path = "selection_fixture.rs"]
mod selection;

struct Fixture {
    child: Child,
    output: Receiver<String>,
}
impl Fixture {
    fn start() -> Result<Self, String> {
        let mut child = Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/fixtures/gtk_accessible.py"
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().ok_or("missing fixture stdout")?;
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let result = Self { child, output };
        let ready = result.read()?;
        if ready["ready"] != true {
            return Err("GTK fixture did not become ready".into());
        }
        Ok(result)
    }
    fn read(&self) -> Result<Value, String> {
        let line = self
            .output
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        serde_json::from_str(&line).map_err(|e| e.to_string())
    }
    fn state(&mut self) -> Result<Value, String> {
        self.command("state")
    }
    fn command(&mut self, command: &str) -> Result<Value, String> {
        writeln!(
            self.child.stdin.as_mut().ok_or("fixture stdin closed")?,
            "{command}"
        )
        .map_err(|e| e.to_string())?;
        self.read()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn request(
    obs: &Observation,
    name: &str,
    action: ActionKind,
    parameters: Value,
) -> Result<DispatchRequest, String> {
    let node = obs
        .nodes
        .iter()
        .find(|n| n.name == name)
        .ok_or_else(|| format!("missing observed node {name}"))?;
    Ok(DispatchRequest {
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
        target: ActionTarget::Element {
            element_ref: node.node_ref.clone(),
        },
        parameters,
        scope: ActionScope::Directed,
    })
}

pub fn run_semantic_selftest() -> Result<(), String> {
    if std::env::var("GROK_CU_X11_FIXTURE").as_deref() != Ok("owned-xvfb") {
        return Err("owned Xvfb required".into());
    }
    crate::require_native_x11()?;
    let mut fixture = Fixture::start()?;
    let adapter = Arc::new(LinuxAdapter::new());
    let deadline = Instant::now() + Duration::from_secs(8);
    let observation = loop {
        let targets = adapter.list_targets()?;
        let Some(target) = targets
            .into_iter()
            .find(|t| t.title == "CU owned accessibility fixture")
        else {
            if Instant::now() >= deadline {
                return Err("GTK window not listed before readiness deadline".into());
            }
            std::thread::sleep(Duration::from_millis(30));
            continue;
        };
        let observation = adapter.observe_for_run("owned-atspi", &target.target_id)?;
        if observation
            .nodes
            .iter()
            .any(|n| n.name == "CU editable field")
        {
            break observation;
        }
        if Instant::now() >= deadline {
            let (g, pid, title) = adapter.with_client(|c| c.semantic_binding(&target.target_id))?;
            let diagnostic = adapter.accessibility.lock().observe(
                "owned-atspi",
                &target.target_id,
                "diagnostic-only",
                g,
                pid,
                &title,
            );
            return Err(format!(
                "real GTK AT-SPI tree was not observed: {}",
                diagnostic
                    .err()
                    .unwrap_or_else(|| "missing named control".into())
            ));
        }
        std::thread::sleep(Duration::from_millis(30)); // Read-only toolkit registration, never action retry.
    };
    println!("PASS real GTK tree bound through XRes and unique accessibility bus owner");
    adapter.with_client(|c| {
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
        use x11rb::wrapper::ConnectionExt as _;
        let geometry = c.geometry(&observation.target_id)?;
        let atom = c
            .conn
            .intern_atom(false, b"_NET_WM_PID")
            .map_err(crate::client::err)?
            .reply()
            .map_err(crate::client::err)?
            .atom;
        c.conn
            .change_property32(
                PropMode::REPLACE,
                geometry.window,
                atom,
                AtomEnum::CARDINAL,
                &[std::process::id()],
            )
            .map_err(crate::client::err)?
            .check()
            .map_err(crate::client::err)?;
        if c.owner_pid(geometry.window)? != fixture.child.id() {
            return Err("writable window PID property overrode native resource owner".into());
        }
        Ok(())
    })?;
    println!("PASS forged window PID property cannot redirect accessibility binding");
    if observation
        .nodes
        .iter()
        .any(|n| n.name == "CU protected field" || n.name.contains("FIXTURE-PROTECTED-TEXT"))
    {
        return Err("protected entry leaked into semantic tree".into());
    }
    println!("PASS protected field exposes no semantic text or action reference");
    let mut write = request(
        &observation,
        "CU editable field",
        ActionKind::SetValue,
        json!({"value":"中文🙂测试"}),
    )?;
    let before_foreign = fixture.state()?;
    let mut foreign = write.clone();
    foreign.run_id = "foreign-unobserved-run".into();
    foreign.parameters = json!({"text":"FOREIGN RUN MUST NOT APPLY"});
    if adapter.act(&foreign).is_ok() || fixture.state()? != before_foreign {
        return Err("foreign run reused another run's observed AT-SPI authority".into());
    }
    println!("PASS another run cannot reuse observed native AT-SPI authority");
    let mut clipboard = write.clone();
    clipboard.action = ActionKind::TypeText;
    clipboard.parameters = json!({"text":"NO SILENT AX FALLBACK", "via":"clipboard"});
    let before_clipboard = fixture.state()?;
    let rejected = adapter.act(&clipboard);
    if !matches!(rejected, Err(ref error) if error.contains("clipboard paste is not yet available"))
        || fixture.state()? != before_clipboard
        || !adapter.is_idle(&clipboard.run_id)
    {
        return Err(
            "explicit clipboard request silently edited native text or retained occupancy".into(),
        );
    }
    println!("PASS explicit clipboard path never silently substitutes AX editing");
    let result = adapter.act(&write)?;
    if !result.postcondition_ok || fixture.state()?["text"] != "中文🙂测试" {
        return Err("Unicode replacement did not match independent GTK readback".into());
    }
    println!("PASS Chinese/emoji replacement and exact independent native readback");
    fixture.command("caret-middle")?;
    write.action = ActionKind::TypeText;
    write.parameters = json!({"text":"第二波"});
    let result = adapter.act(&write)?;
    let inserted = fixture.state()?["text"]
        .as_str()
        .ok_or("missing GTK text")?
        .to_owned();
    if !result.postcondition_ok || inserted != "中文第二波🙂测试" {
        return Err("Unicode insertion used incorrect character/byte offsets".into());
    }
    println!("PASS Unicode caret insertion without keyboard remap or clipboard");
    let selected = fixture.command("select-middle")?;
    if selected["selection"] != json!([2, 6]) {
        return Err("GTK fixture did not select the intended Unicode span".into());
    }
    write.parameters = json!({"text":"替换🦀"});
    let result = adapter.act(&write)?;
    let selected_result = fixture.state()?;
    if !result.postcondition_ok
        || selected_result["text"] != "中文替换🦀测试"
        || selected_result["caret"] != 5
        || selected_result["selection"] != json!([])
    {
        return Err(format!(
            "selected Unicode replacement was not exact: {selected_result}"
        ));
    }
    println!("PASS selected Unicode span replaces in place with exact caret and untouched suffix");
    let value = selected_result["text"]
        .as_str()
        .ok_or("missing GTK text")?
        .to_owned();
    let click = request(&observation, "CU increment", ActionKind::Click, json!({}))?;
    adapter.act(&click)?;
    if fixture.state()?["clicks"] != 1 {
        return Err("semantic click did not apply exactly once".into());
    }
    println!("PASS observed semantic click applies once to the native control");
    let preview = adapter.capture_for_run(
        "preview-only",
        &observation.target_id,
        grok_computer_use_core::adapter::CaptureOptions {
            managed_request: None,
            for_model: false,
            screenshot: true,
            cancellation: Default::default(),
        },
    )?;
    if preview.image.png_base64.is_none() || !preview.nodes.is_empty() {
        return Err("preview either lacks pixels or minted input references".into());
    }
    write.action = ActionKind::SetValue;
    write.parameters = json!({"value":value});
    adapter.act(&write)?;
    if fixture.state()?["text"] != value {
        return Err("preview invalidated the model snapshot".into());
    }
    println!("PASS UI preview retains pixels without minting or retiring model references");
    fixture.command("disable")?;
    let disabled = request(
        &observation,
        "CU editable field",
        ActionKind::SetValue,
        json!({"value":"MUST NOT APPLY"}),
    )?;
    if adapter.act(&disabled).is_ok() || fixture.state()?["text"] != value {
        return Err("disabled GTK input changed".into());
    }
    fixture.command("enable")?;
    println!("PASS disabled native control rejects stale advertised actions with zero effect");
    fixture.command("focus-away")?;
    if adapter.act(&disabled).is_ok() || fixture.state()?["text"] != value {
        return Err("focus drift permitted semantic input".into());
    }
    fixture.command("focus-back")?;
    println!("PASS another native window's focus blocks semantic input without raising target");
    let mut cancelled = request(
        &observation,
        "CU editable field",
        ActionKind::SetValue,
        json!({"value":"MUST NOT APPLY"}),
    )?;
    cancelled.cancellation.cancel();
    if adapter.act(&cancelled).is_ok() || fixture.state()?["text"] != value {
        return Err("cancelled text changed GTK".into());
    }
    cancelled.cancellation = Default::default();
    cancelled.target = ActionTarget::Element {
        element_ref: "unobserved-control".into(),
    };
    if adapter.act(&cancelled).is_ok() || fixture.state()?["text"] != value {
        return Err("guessed reference changed GTK".into());
    }
    let _new = adapter.observe_for_run("owned-atspi", &observation.target_id)?;
    if adapter.act(&write).is_ok() || fixture.state()?["text"] != value {
        return Err("retired snapshot changed GTK".into());
    }
    if !adapter.is_idle("owned-atspi") {
        return Err("completed native input did not release ownership".into());
    }
    println!("PASS cancellation, guessed references and retired snapshots have zero effect");
    let before_replacement = adapter.observe_for_run("owned-atspi", &observation.target_id)?;
    let old_control = request(
        &before_replacement,
        "CU editable field",
        ActionKind::SetValue,
        json!({"value":"WRONG INSTANCE"}),
    )?;
    fixture.command("replace")?;
    if adapter.act(&old_control).is_ok() || fixture.state()?["text"] != "replacement" {
        return Err("destroyed control transferred authority to replacement".into());
    }
    println!("PASS same-name replacement control never inherits old reference authority");
    selection::run(&adapter, &mut fixture, &observation.target_id)?;
    recovery::run(&adapter, &mut fixture, &observation.target_id)?;
    wait_acceptance(&adapter, &mut fixture, &observation.target_id)?;
    // Exercise the actual production Broker's coordinate-to-semantic routing,
    // not just direct adapter dispatch. Authorization is fixture-only here.
    use grok_computer_use_core::broker::{BrokerOptions, ComputerUseBroker};
    use grok_computer_use_core::protocol::{ActionRequest, PROTOCOL_VERSION};
    let broker = ComputerUseBroker::new(
        adapter.clone(),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir()
                .join(format!("cu-owned-atspi-{}.lease", uuid::Uuid::new_v4())),
            ..Default::default()
        },
    );
    broker
        .open_run("owned-fixture", "broker-atspi")
        .map_err(|e| e.to_string())?;
    let generation = broker
        .authorize_target("broker-atspi", &observation.target_id)
        .map_err(|e| e.to_string())?;
    let observed = broker.observe("broker-atspi").map_err(|e| e.to_string())?;
    let button = observed
        .nodes
        .iter()
        .find(|n| n.name == "CU increment" && n.actions.iter().any(|a| a == "click"))
        .ok_or("missing button in Broker observation")?;
    let outcome = broker.act(ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "broker-primary-click".into(),
        run_id: "broker-atspi".into(),
        target_id: observed.target_id.clone(),
        target_generation: generation,
        snapshot_id: observed.snapshot_id,
        geometry_revision: observed.geometry_revision,
        action: ActionKind::Click,
        target: ActionTarget::Coord {
            x: button.x.ok_or("button x")? + button.width.ok_or("button width")? / 2.0,
            y: button.y.ok_or("button y")? + button.height.ok_or("button height")? / 2.0,
        },
        parameters: json!({}),
    });
    if !outcome.executed || fixture.state()?["clicks"] != 2 {
        return Err(format!(
            "Broker did not route exact actionable native node: {:?}",
            outcome.reason
        ));
    }
    println!(
        "PASS production Broker coordinate routing selects actionable control, not enclosing root"
    );
    let observed = broker.observe("broker-atspi").map_err(|e| e.to_string())?;
    let label = observed
        .nodes
        .iter()
        .find(|n| n.name == "CU wait pending")
        .ok_or("Broker did not observe the Wait label")?;
    fixture.command("wait-start")?;
    let mut wait_request = ActionRequest {
        version: PROTOCOL_VERSION,
        action_id: "broker-native-wait".into(),
        run_id: "broker-atspi".into(),
        target_id: observed.target_id,
        target_generation: generation,
        snapshot_id: observed.snapshot_id,
        geometry_revision: observed.geometry_revision,
        action: ActionKind::Wait,
        target: ActionTarget::Element {
            element_ref: label.node_ref.clone(),
        },
        parameters: json!({"nameEquals":"CU wait 完成🙂","timeoutMs":2000}),
    };
    let outcome = broker.act(wait_request.clone());
    if outcome.kind != grok_computer_use_core::protocol::OutcomeKind::Verified
        || fixture.state()?["status"] != "CU wait 完成🙂"
        || fixture.state()?["clicks"] != 2
    {
        return Err(format!(
            "Broker did not dispatch retained native Wait: {:?}",
            outcome.reason
        ));
    }
    println!("PASS production Broker verifies real delayed native Wait without replacing the model reference");
    let before = fixture.state()?;
    wait_request.action_id = "broker-native-timeout".into();
    wait_request.parameters = json!({"nameEquals":"never exists","timeoutMs":100});
    let timeout = broker.act(wait_request.clone());
    if timeout.kind != grok_computer_use_core::protocol::OutcomeKind::Rejected || timeout.executed {
        return Err(format!(
            "read-only Broker timeout was not a known rejection: {:?}",
            timeout.kind
        ));
    }
    wait_request.action_id = "broker-native-after-timeout".into();
    wait_request.parameters = json!({"nameEquals":"CU wait 完成🙂","timeoutMs":2000});
    if broker.act(wait_request).kind != grok_computer_use_core::protocol::OutcomeKind::Verified
        || fixture.state()? != before
    {
        return Err(
            "Broker timeout destroyed the valid native reference or changed the application".into(),
        );
    }
    println!("PASS Broker Wait timeout is rejected without effects and the same reference remains usable");
    selection::broker_acceptance(&broker, &mut fixture, &observation.target_id)?;
    recovery::broker_acceptance(&broker, &adapter, &mut fixture)?;
    recovery::transport_loss(&adapter, &mut fixture, &observation.target_id)?;
    Ok(())
}

fn wait_acceptance(
    adapter: &Arc<LinuxAdapter>,
    fixture: &mut Fixture,
    target: &str,
) -> Result<(), String> {
    let caps = adapter.capabilities();
    if !caps.wait.semantic || caps.wait.coordinate || !adapter.wait_uses_retained_reference() {
        return Err("native retained-reference Wait capability is missing".into());
    }
    let observe = || adapter.observe_for_run("owned-atspi-wait", target);
    let obs = observe()?;
    let mut req = request(
        &obs,
        "CU wait pending",
        ActionKind::Wait,
        json!({"nameEquals":"CU wait pending","timeoutMs":2000}),
    )?;
    let before = fixture.state()?;
    let result = adapter.act(&req)?;
    if result.applied
        || !result.postcondition_ok
        || !result.verifiable
        || fixture.state()? != before
    {
        return Err("immediate native Wait must be verified and read-only".into());
    }
    println!(
        "PASS Wait advertises semantic-only and reads the original native object without input"
    );
    req.parameters = json!({"nameEquals":"CU wait 完成🙂","timeoutMs":2000});
    fixture.command("wait-start")?;
    let result = adapter.act(&req)?;
    let after = fixture.state()?;
    if result.applied
        || !result.postcondition_ok
        || after["status"] != "CU wait 完成🙂"
        || after["statusEvents"].as_u64() != before["statusEvents"].as_u64().map(|n| n + 1)
        || after["text"] != before["text"]
        || after["clicks"] != before["clicks"]
    {
        return Err("Wait did not observe the GTK timer's actual Unicode label transition".into());
    }
    println!("PASS Wait follows real GTK timed Unicode name change on the same retained object");
    // A read-only Wait does not need focus and does not raise or enable a control.
    fixture.command("wait-disable")?;
    fixture.command("focus-away")?;
    let result = adapter.act(&req)?;
    if result.applied || !result.postcondition_ok || adapter.foreground_input_available(target) {
        return Err("background Wait raised the target or failed the public label read".into());
    }
    fixture.command("focus-back")?;
    println!("PASS disabled/background public-label Wait is read-only and never steals focus");
    // The fixture's focus restoration can restack its X window. Establish the
    // actual current geometry epoch before testing the independent timeout.
    let timeout_obs = observe()?;
    req = request(
        &timeout_obs,
        "CU wait 完成🙂",
        ActionKind::Wait,
        json!({"nameEquals":"never exists","timeoutMs":100}),
    )?;
    let before_timeout = fixture.state()?;
    req.parameters = json!({"nameEquals":"never exists","timeoutMs":100});
    let started = Instant::now();
    let error = adapter.act(&req).expect_err("missing name must time out");
    if !error.contains("timed out")
        || started.elapsed() > Duration::from_millis(1600)
        || fixture.state()? != before_timeout
    {
        return Err(format!(
            "Wait deadline or zero-effect check failed: {error}"
        ));
    }
    println!("PASS missing-name Wait times out without late success or application effects");
    for p in [
        json!({}),
        json!({"nameEquals":"x","timeoutMs":0}),
        json!({"nameEquals":"x","timeoutMs":10001}),
        json!({"nameEquals":"x","extra":true}),
    ] {
        req.parameters = p;
        if adapter.act(&req).is_ok() {
            return Err("invalid Wait parameters accepted".into());
        }
    }
    req.parameters = json!({"nameEquals":"CU wait 完成🙂","timeoutMs":2000});
    let mut foreign = req.clone();
    foreign.run_id = "foreign-wait".into();
    if adapter.act(&foreign).is_ok() {
        return Err("foreign run inherited Wait reference".into());
    }
    let mut guessed = req.clone();
    guessed.target = ActionTarget::Element {
        element_ref: "guessed".into(),
    };
    if adapter.act(&guessed).is_ok() {
        return Err("unobserved Wait reference accepted".into());
    }
    let mut desktop = req.clone();
    desktop.scope = ActionScope::Desktop;
    if adapter.act(&desktop).is_ok() {
        return Err("desktop fallback accepted by Wait".into());
    }
    let mut cancelled = req.clone();
    // Cancellation tokens are shared by Clone; do not cancel the valid request
    // subsequently used to prove visibility/identity rejection.
    cancelled.cancellation = Default::default();
    cancelled.cancellation.cancel();
    if adapter.act(&cancelled).is_ok() {
        return Err("cancelled Wait succeeded".into());
    }
    if fixture.state()? != before_timeout {
        return Err("rejected Wait changed the application".into());
    }
    println!(
        "PASS invalid, foreign, guessed, desktop and cancelled Wait requests have zero effect"
    );
    fixture.command("wait-hide")?;
    let hidden = adapter
        .act(&req)
        .expect_err("hidden Wait object must reject");
    if hidden.contains("cancel") {
        return Err("hidden-control acceptance accidentally used a cancelled request".into());
    }
    fixture.command("wait-reset")?;
    println!("PASS hidden native Wait control cannot satisfy the old observed reference");
    let obs = observe()?;
    req = request(
        &obs,
        "CU wait pending",
        ActionKind::Wait,
        json!({"nameEquals":"CU wait 完成🙂","timeoutMs":2000}),
    )?;
    fixture.command("wait-replace")?;
    if adapter.act(&req).is_ok() {
        return Err("replacement label inherited retained Wait reference".into());
    }
    if fixture.state()?["status"] != "CU wait 完成🙂" {
        return Err("GTK did not create the replacement label".into());
    }
    println!("PASS same-name/same-position replacement label never satisfies retained Wait");
    // Poll the actual NativeActionSlot before Stop/revoke/reobserve, rather than
    // assuming the worker started based on a sleep or a log file.
    for mode in ["stop", "release", "replace-observation"] {
        fixture.command("wait-reset")?;
        let obs = observe()?;
        let req = request(
            &obs,
            "CU wait pending",
            ActionKind::Wait,
            json!({"nameEquals":"never exists","timeoutMs":10000}),
        )?;
        let worker_req = req.clone();
        let worker_adapter = Arc::clone(adapter);
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = tx.send(worker_adapter.act(&worker_req));
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while adapter.is_idle(&req.run_id) {
            if Instant::now() >= deadline {
                return Err("Wait worker did not acquire native ownership".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let queried = Instant::now();
        if !adapter.capabilities().wait.semantic || queried.elapsed() > Duration::from_millis(200) {
            return Err("capability query blocked behind the active Wait".into());
        }
        let mut overlapping = req.clone();
        overlapping.action_id = "overlap-must-not-run".into();
        if adapter.act(&overlapping).is_ok() {
            return Err("overlapping Wait acquired input ownership".into());
        }
        let stopped = Instant::now();
        match mode {
            "stop" => {
                adapter.abort(&req.run_id, req.generation + 1)?;
                if stopped.elapsed() > Duration::from_millis(150) {
                    return Err("Stop blocked on remote AT-SPI reads".into());
                }
            }
            "release" => adapter.release_target_for_run(&req.run_id, target),
            _ => {
                observe()?;
            }
        }
        let result = rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        worker.join().map_err(|_| "Wait worker panicked")?;
        let mut stale = req.clone();
        stale.cancellation = Default::default();
        if result.is_ok()
            || !adapter.is_idle(&req.run_id)
            || stopped.elapsed() > Duration::from_secs(3)
            || adapter.act(&stale).is_ok()
        {
            return Err(format!(
                "{mode} did not retire the active Wait and release ownership"
            ));
        }
        println!("PASS active native Wait {mode}: bounded retirement, idle readback, no reference revival");
    }
    Ok(())
}
