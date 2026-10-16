//! Native external-oracle contract; compiled on Windows, never inferred from fakes.
#![cfg(windows)]
use gcu_core::{
    driver::*,
    model::{NativeHandle, RawNode},
};
use gcu_driver_windows::{
    api::WinApi,
    worker::{ExecMode, WorkerConfig},
    RealWinApi, WindowsDriver,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn oracle(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("fixture oracle file")).unwrap()
}
async fn expect_oracle(path: &Path, field: &str, expected: Value) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if oracle(path)[field] == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("external fixture state must change after exactly one delivery");
}
fn find(node: &RawNode, id: &str) -> Option<NativeHandle> {
    if node.identifier.as_deref() == Some(id) || node.name.as_deref() == Some(id) {
        return Some(node.handle.clone());
    }
    node.children.iter().find_map(|n| find(n, id))
}
async fn handle(driver: &WindowsDriver, win: &WindowRef, id: &str) -> NativeHandle {
    let raw = driver
        .snapshot_tree(win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    find(&raw.root.unwrap(), id).expect("fixture must expose the exact named control")
}
fn foreground(api: &RealWinApi) -> u64 {
    api.enum_windows()
        .unwrap()
        .into_iter()
        .find(|w| w.foreground)
        .expect("keep a visible unrelated window in front")
        .hwnd
}

#[tokio::test]
#[ignore = "not_run: requires disposable Windows desktop, fixture oracle, unrelated foreground window and GCU_WORKER_EXE"]
async fn semantic_actions_native_external_oracle_and_background_contract() {
    semantic_contract(ExecMode::default()).await;
}

#[tokio::test]
#[ignore = "not_run: requires disposable unelevated Windows desktop, fixture oracle and unrelated foreground window"]
async fn semantic_actions_in_process_native_external_oracle_and_background_contract() {
    semantic_contract(ExecMode::InProcess).await;
}

async fn semantic_contract(mode: ExecMode) {
    let exe = PathBuf::from(
        std::env::var_os("GCU_FIXTURE_EXE").expect("absolute path to running gcu-fixture.exe"),
    );
    assert!(exe.is_absolute());
    assert_eq!(
        exe.file_name().unwrap().to_string_lossy(),
        "gcu-fixture.exe"
    );
    let state = PathBuf::from(
        std::env::var_os("GCU_FIXTURE_STATE").expect("state file of that synthetic fixture"),
    );
    let worker = if mode == ExecMode::Worker {
        let path = PathBuf::from(
            std::env::var_os("GCU_WORKER_EXE").expect("absolute path to built gcu.exe"),
        );
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap().to_string_lossy(), "gcu.exe");
        path
    } else {
        PathBuf::from("gcu") // InProcess never launches this executable.
    };
    let driver = if mode == ExecMode::Worker {
        WindowsDriver::monitored(
            WorkerConfig::new(worker),
            Default::default(),
            std::sync::Arc::new(crate::common::TestStop::default()),
        )
        .await
        .unwrap()
    } else {
        WindowsDriver::new_in_process().await.unwrap()
    };
    let app = driver
        .resolve_app(&AppQuery(exe.to_string_lossy().into()))
        .await
        .unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let api = RealWinApi::new().unwrap();
    let front = foreground(&api);
    let target_pid = win.app.info.pid.unwrap();
    assert!(
        !api.enum_windows()
            .unwrap()
            .iter()
            .any(|w| w.pid == target_pid && w.foreground),
        "place another window in front before running this test"
    );
    let initial = oracle(&state);
    for (id, value, expected_field, expected) in [
        (
            "name_field",
            Some("UIA fixture 中🦀"),
            "text",
            json!("UIA fixture 中🦀"),
        ),
        ("slider_field", Some("37"), "slider", json!(37.0)),
        (
            "agree_checkbox",
            None,
            "checked",
            json!(!initial["checked"].as_bool().unwrap()),
        ),
        (
            "counter_button",
            None,
            "counter",
            json!(initial["counter"].as_u64().unwrap() + 1),
        ),
    ] {
        let el = handle(&driver, &win, id).await;
        let action = if let Some(value) = value {
            DriverAction::SetValue {
                el,
                value: value.into(),
            }
        } else {
            DriverAction::Press { el }
        };
        assert_eq!(
            driver.perform(&action, &Cancel::default()).await.unwrap(),
            if id == "counter_button" {
                Delivery::TargetedEvent
            } else {
                Delivery::Accessibility
            }
        );
        expect_oracle(&state, expected_field, expected).await;
        assert_eq!(
            foreground(&api),
            front,
            "semantic actions must preserve the foreground HWND"
        );
    }
    let el = handle(&driver, &win, "password_field").await;
    assert_eq!(
        driver
            .perform(
                &DriverAction::SetValue {
                    el,
                    value: "synthetic-only".into()
                },
                &Cancel::default()
            )
            .await,
        Err(DriverError::NotSettable {
            maybe_executed: false
        })
    );
    assert_eq!(oracle(&state)["password_len"], initial["password_len"]);
    // The file is authoritative; a successful COM return alone cannot pass C05.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        oracle(&state)["counter"].as_u64().unwrap(),
        initial["counter"].as_u64().unwrap() + 1
    );
    assert_eq!(foreground(&api), front);
}
