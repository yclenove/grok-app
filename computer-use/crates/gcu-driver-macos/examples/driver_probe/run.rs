use gcu_core::{
    driver::{Cancel, Driver, TreeOpts, WindowRef},
    model::{RawNode, RawTree, Role},
};
use gcu_driver_macos::{
    diagnostics::{self, FixtureToggles},
    input::hotkey::{HotkeyConfig, SharedStopLatch, StopLatch},
    MacosDriver,
};
use serde_json::json;
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

#[path = "actions.rs"]
mod actions;
#[path = "discovery.rs"]
mod discovery;
#[path = "select.rs"]
mod select;

pub(super) fn emit(kind: &str, data: serde_json::Value) {
    println!("{}", json!({"kind": kind, "data": data}));
}

pub async fn run(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    emit(
        "START",
        json!({"fixture": path, "pid": std::process::id(),
        "format": "one JSON object per line; grep kind/AX_CALL/AX_FALLBACK/CANCEL_REASON",
        "actions": "fixture only; semantic actions; may open/activate fixture once if no window; no clipboard or CG posting"}),
    );
    // This explicitly invoked disposable fixture Host starts resumed. The
    // production constructor and persistent Host latch remain fail-closed.
    let latch = Arc::new(SharedStopLatch::new(false));
    let started = Instant::now();
    let driver = MacosDriver::with_stop_latch(latch.clone(), HotkeyConfig::default(), 3000).await?;
    emit(
        "DRIVER",
        json!({"elapsed_ms": started.elapsed().as_secs_f64()*1000.0,
        "initial_stop": latch.is_stopped(), "reason": "explicit fixture Host starts resumed; with_stop_latch installs RealEvents + native probe"}),
    );
    println!("{}", diagnostics::runtime_report(&driver).await?);
    let doctor = driver.doctor().await;
    emit(
        "DOCTOR",
        json!({"ready": doctor.ready, "issues": doctor.issues,
        "capabilities": doctor.capabilities}),
    );
    let win = discovery::window(&driver, path).await?;
    let opts = TreeOpts {
        max_tree_nodes: 5000,
        ..TreeOpts::default()
    };
    // Observer construction/registration is asynchronous on its own AX thread.
    // Native AX_CALL records report every registration code; Some(stream)
    // alone is not evidence that any notification registered successfully.
    let _events = driver.subscribe(&win);
    emit(
        "SUBSCRIBE",
        json!({"stream_returned": _events.is_some(),
        "reason": "inspect AXObserverCreate/AddNotification return codes; no event expected without a UI change"}),
    );
    emit(
        "TREE_OPTIONS",
        json!({"timeout_ms": opts.timeout.as_millis(),
        "max_tree_nodes": opts.max_tree_nodes, "max_tree_depth": opts.max_tree_depth,
        "text_limit": opts.text_limit, "native_walk_ms": 2000,
        "trace_overhead": "synchronous JSON output included in elapsed time"}),
    );
    emit("FIXTURE_STATE_INITIAL", actions::read_state().await);
    let baseline = snapshot(&driver, &win, &opts, "baseline").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let repeat = snapshot(&driver, &win, &opts, "baseline_repeat_500ms").await;
    let toggles = FixtureToggles::new(&driver, win.app.info.pid.ok_or("fixture has no pid")?)?;
    if let Err(error) = toggles.enable().await {
        emit(
            "TOGGLE_ERROR",
            json!({"error": error.to_string(), "maybe_executed": error.maybe_executed()}),
        );
    }
    // AccessKit may publish a tree asynchronously after activation. Read both
    // immediately and after an explicit bounded publication interval.
    let immediate = snapshot(&driver, &win, &opts, "toggles_enabled_immediate").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let enabled = snapshot(&driver, &win, &opts, "toggles_enabled_500ms").await;
    emit(
        "TREE_COMPARE",
        json!({"baseline": count_tree(baseline.as_ref()),
        "baseline_repeat_500ms": count_tree(repeat.as_ref()),
        "enabled_immediate": count_tree(immediate.as_ref()),
        "enabled_500ms": count_tree(enabled.as_ref()),
        "interpretation": "growth after switches suggests lazy AccessKit activation; repeated baseline controls for time/order are needed before claiming necessity"}),
    );
    actions::run(&driver, &win, &opts, enabled).await;
    if let Err(error) = toggles.restore().await {
        emit("RESTORE_ERROR", json!({"error": error.to_string()}));
    }
    let restored = snapshot(&driver, &win, &opts, "toggles_restored").await;
    emit(
        "TREE_COMPARE_RESTORED",
        json!({"baseline": count_tree(baseline.as_ref()), "restored": count_tree(restored.as_ref())}),
    );
    driver.release_all_input().await;
    emit("FIXTURE_STATE_FINAL", actions::read_state().await);
    println!("{}", diagnostics::runtime_report(&driver).await?);
    emit(
        "END",
        json!({"elapsed_ms": started.elapsed().as_secs_f64()*1000.0,
        "native_desktop_verdict": "diagnostic facts only; inspect action and fixture-state records"}),
    );
    Ok(())
}

pub(super) async fn snapshot(
    driver: &MacosDriver,
    win: &WindowRef,
    opts: &TreeOpts,
    phase: &str,
) -> Option<RawTree> {
    let started = Instant::now();
    match driver.snapshot_tree(win, opts, &Cancel::default()).await {
        Ok(tree) => {
            emit(
                "TREE",
                json!({"phase": phase, "nodes": count_tree(Some(&tree)),
                "incomplete": tree.incomplete, "elapsed_ms": started.elapsed().as_secs_f64()*1000.0}),
            );
            let nodes = nodes(tree.root.as_ref());
            for (index, node) in nodes.iter().enumerate().take(200) {
                print_node("TREE_NODE", phase, index, node);
            }
            for (index, node) in nodes.iter().enumerate().filter(|(_, n)| {
                matches!(
                    n.role,
                    Role::Button
                        | Role::CheckBox
                        | Role::ComboBox
                        | Role::List
                        | Role::Slider
                        | Role::TextField
                        | Role::PopUpButton
                        | Role::ScrollArea
                        | Role::TextArea
                        | Role::SearchField
                )
            }) {
                print_node("CONTROL", phase, index, node);
            }
            Some(tree)
        }
        Err(error) => {
            emit(
                "TREE_ERROR",
                json!({"phase": phase, "error": error.to_string(),
                "elapsed_ms": started.elapsed().as_secs_f64()*1000.0}),
            );
            None
        }
    }
}

pub(super) fn nodes(root: Option<&RawNode>) -> Vec<&RawNode> {
    let mut out = Vec::new();
    let mut stack: Vec<_> = root.into_iter().collect();
    while let Some(node) = stack.pop() {
        out.push(node);
        stack.extend(node.children.iter().rev());
    }
    out
}

fn count_tree(tree: Option<&RawTree>) -> Option<usize> {
    tree.map(|t| nodes(t.root.as_ref()).len())
}

pub(super) fn first_button(root: Option<&RawNode>) -> Option<&RawNode> {
    nodes(root).into_iter().find(|n| {
        n.role == Role::Button
            && !n.subrole.as_deref().is_some_and(|s| {
                matches!(
                    s,
                    "AXCloseButton" | "AXMinimizeButton" | "AXZoomButton" | "AXFullScreenButton"
                )
            })
    })
}

fn print_node(kind: &str, phase: &str, index: usize, node: &RawNode) {
    emit(
        kind,
        json!({"phase": phase, "index": index, "role": node.role,
        "native_role": node.native_role, "subrole": node.subrole,
        "name": node.name, "identifier": node.identifier, "description": node.description,
        "states": format!("{:?}", node.states), "frame": node.frame,
        "actions": node.actions, "children": node.children.len()}),
    );
}
