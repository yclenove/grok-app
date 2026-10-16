use super::{emit, first_button, nodes, snapshot};
use gcu_core::{
    driver::{Cancel, Driver, DriverAction, DriverError, TreeOpts, WindowRef},
    model::{RawTree, Role, States},
};
use gcu_driver_macos::MacosDriver;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub(super) async fn run(
    driver: &MacosDriver,
    win: &WindowRef,
    opts: &TreeOpts,
    tree: Option<RawTree>,
) {
    let root = tree.as_ref().and_then(|t| t.root.as_ref());
    if let Some(button) = first_button(root) {
        emit(
            "ACTION_TARGET",
            json!({"action": "press_first_button", "name": button.name,
            "identifier": button.identifier, "subrole": button.subrole,
            "note": "first content Button; native close/minimize/zoom/fullscreen controls excluded"}),
        );
        perform(
            driver,
            "press_first_button",
            DriverAction::Press {
                el: button.handle.clone(),
            },
        )
        .await;
    } else {
        skip("press_first_button", "no content Button found");
    }
    // Refresh handles for each action: opening another snapshot invalidates all
    // previous native ids. Do not reuse handles from the baseline comparison.
    let tree = snapshot(driver, win, opts, "before_counter").await;
    let counter = tree.as_ref().and_then(|t| {
        nodes(t.root.as_ref()).into_iter().find(|n| {
            n.role == Role::Button
                && (n.identifier.as_deref() == Some("counter_button")
                    || n.name
                        .as_deref()
                        .is_some_and(|s| s.contains("counter_button") || s.contains("Increment")))
        })
    });
    if let Some(button) = counter {
        perform(
            driver,
            "press_counter",
            DriverAction::Press {
                el: button.handle.clone(),
            },
        )
        .await;
    } else {
        skip(
            "press_counter",
            "counter_button/Increment missing; no guessed substitute",
        );
    }
    for (role, label, value) in [
        (Role::TextField, "set_text_field", "gcu-driver-probe"),
        (Role::CheckBox, "press_checkbox", ""),
        (Role::Slider, "set_slider", "41"),
    ] {
        let tree = snapshot(driver, win, opts, label).await;
        let node = tree.as_ref().and_then(|t| {
            nodes(t.root.as_ref())
                .into_iter()
                .find(|n| n.role == role && !n.states.contains(States::PASSWORD))
        });
        if let Some(node) = node {
            emit(
                "ACTION_TARGET",
                json!({"action": label, "name": node.name,
                "identifier": node.identifier, "states": format!("{:?}", node.states)}),
            );
            let req = if role == Role::CheckBox {
                DriverAction::Press {
                    el: node.handle.clone(),
                }
            } else {
                DriverAction::SetValue {
                    el: node.handle.clone(),
                    value: value.into(),
                }
            };
            perform(driver, label, req).await;
        } else {
            skip(
                label,
                "requested control missing (password fields excluded)",
            );
        }
    }
}

fn skip(action: &str, reason: &str) {
    emit("ACTION_SKIP", json!({"action": action, "reason": reason}));
}

pub(super) async fn perform(driver: &MacosDriver, label: &str, req: DriverAction) {
    let before = read_state().await;
    let cancel = Cancel::default();
    let guards_before = guard_facts(driver);
    emit(
        "ACTION_START",
        json!({"action": label, "guards": guards_before,
        "cancel_token": cancel.is_cancelled(), "timeout_ms": 4750}),
    );
    let started = Instant::now();
    let result = driver.perform(&req, &cancel).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (cancelled, maybe, error, delivery) = match result {
        Ok(delivery) => (false, false, None, Some(format!("{delivery:?}"))),
        Err(error) => (
            matches!(error, DriverError::Cancelled { .. }),
            error.maybe_executed(),
            Some(format!("{error:?}: {error}")),
            None,
        ),
    };
    emit(
        "ACTION_RESULT",
        json!({"action": label, "elapsed_ms": elapsed_ms,
        "cancelled": cancelled, "maybe_executed": maybe, "error": error, "delivery": delivery,
        "guards_after": guard_facts(driver), "cancel_token": cancel.is_cancelled(),
        "cancel_reason": if cancelled && guards_before["stopped"] == true { "host_stop_before_dispatch" }
            else if cancelled && cancel.is_cancelled() { "caller_cancel_token" }
            else if cancelled { "Stop changed during dispatch; inspect CANCEL_REASON and AX_CALL records" }
            else { "none" }}),
    );
    // A returned AX success only acknowledges delivery. Observe asynchronous
    // fixture persistence for up to 1.5s, without replaying any action.
    let key = if label.contains("press_") && label != "press_checkbox" {
        "counter"
    } else if label == "press_checkbox" {
        "checkbox"
    } else if label == "set_slider" {
        "slider"
    } else {
        "text"
    };
    let deadline = Instant::now() + Duration::from_millis(1500);
    let after = loop {
        let state = read_state().await;
        if state[key] != before[key] || cancelled || Instant::now() >= deadline {
            break state;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let changed = before.get(key).zip(after.get(key)).map(|(b, a)| b != a);
    emit(
        "FIXTURE_EFFECT",
        json!({"action": label, "key": key, "before": before,
        "after": after, "changed": changed,
        "counter_delta": after["counter"].as_i64().zip(before["counter"].as_i64()).map(|(a,b)| a-b),
        "note": "null means unavailable; first Button may not be the counter; press_counter is a separate explicit call"}),
    );
}

fn guard_facts(driver: &MacosDriver) -> Value {
    driver
        .input_guards()
        .map(|g| {
            json!({"stopped": g.is_stopped(),
        "stop_available": g.stop_available(), "activity_age_ms": g.ms_since_physical_input()})
        })
        .unwrap_or(Value::Null)
}

pub(super) async fn read_state() -> Value {
    tokio::task::spawn_blocking(|| {
        let result = (|| -> Result<Value, Box<dyn std::error::Error>> {
            let path = std::env::var("GCU_FIXTURE_STATE")?;
            let state: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
            // Whitelist fixture oracle fields; never print event text/passwords.
            Ok(json!({"path": path, "counter": state["counter"], "text": state["text"],
                "checkbox": state["checkbox"], "slider": state["slider"],
                "combo_selected": state["combo_selected"], "list_visible_range": state["list_visible_range"],
                "tab": state["tab"]}))
        })();
        match result { Ok(value) => value, Err(error) => json!({"state_error": error.to_string()}) }
    }).await.unwrap_or_else(|error| json!({"state_error": error.to_string()}))
}
