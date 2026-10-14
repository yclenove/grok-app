//! Opt-in native UIA rendering on an unelevated, disposable Windows desktop.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{
    driver::{AppQuery, Cancel, Driver, TreeOpts, WindowRef},
    render::{render_text, RenderOpts},
    tree::Pipeline,
};
use gcu_driver_windows::WindowsDriver;

async fn render_open_app(query: &str) {
    // Open only system apps on the disposable desktop before invoking these tests.
    // No account access, UI input, activation, or app launching by this test.
    let driver = WindowsDriver::new()
        .await
        .expect("unelevated interactive Windows desktop");
    let app = driver
        .resolve_app(&AppQuery::from(query))
        .await
        .expect("open the named system app first");
    let info = driver
        .list_windows(Some(&app))
        .await
        .unwrap()
        .into_iter()
        .next()
        .expect("open a visible window");
    let win = WindowRef {
        app: app.clone(),
        id: info.id.clone(),
    };
    let opts = TreeOpts {
        max_tree_nodes: 5000,
        ..Default::default()
    };
    let raw = driver
        .snapshot_tree(&win, &opts, &Cancel::default())
        .await
        .unwrap();
    assert!(raw.root.is_some());
    let tree = Pipeline::default().run(raw, &TreeOpts::default());
    let text = render_text(&tree, &RenderOpts::new(app.info, info));
    assert!(text.contains("Window:"));
    assert!(!tree.nodes().is_empty());
    let stream = driver
        .subscribe(&win)
        .expect("gated UIA event subscription");
    drop(stream);
    driver.release_all_input().await;
}
#[tokio::test]
#[ignore = "requires open Notepad on a disposable interactive Windows desktop"]
async fn notepad_uia_native_smoke() {
    render_open_app("Notepad").await;
}
#[tokio::test]
#[ignore = "requires an open File Explorer folder on a disposable Windows desktop"]
async fn explorer_uia_native_smoke() {
    render_open_app("explorer").await;
}
#[tokio::test]
#[ignore = "requires open Settings on a disposable interactive Windows desktop"]
async fn settings_uia_native_smoke() {
    render_open_app("SystemSettings").await;
}
