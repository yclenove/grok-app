//! Disposable portable native-source fixture.
use gcu_core::driver::{AppQuery, Driver};
use gcu_driver_windows::{
    capture::{CaptureBackend, CapturePath, FakeCapture, Paint},
    testing::{medium_security, signed_image, window, FakeWinApi},
    uia::{
        cache::{Patterns, Properties},
        events::FakeUiaEvents,
        patterns::Pattern,
        source::{FakeNode, FakeUia},
    },
    WindowsDriver,
};
use std::sync::Arc;

pub async fn driver(namespace: &str) -> Arc<dyn Driver> {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.paths.insert(7, "C:\\Editor.exe".into());
        s.images
            .insert("C:\\Editor.exe".into(), signed_image("C:\\Editor.exe"));
        s.processes.insert(7, medium_security());
        let mut win = window(8, 7);
        // Simulate a per-launcher HWND property namespace. A changed namespace
        // after restart invalidates the old window ID, just like real Win32.
        win.created = namespace
            .bytes()
            .fold(1u64, |n, b| n.wrapping_mul(31).wrapping_add(u64::from(b)));
        win.foreground = true;
        s.windows.push(win);
        s.desktops.insert(8, Some(true));
    });
    let source = Arc::new(FakeUia::new(FakeNode {
        properties: Properties {
            name: Some("worker fixture".into()),
            focused: true,
            read_only: Some(false),
            patterns: Patterns {
                invoke: true,
                toggle: true,
                selection_item: true,
                expand_collapse: true,
                value: true,
                range_value: true,
                scroll: true,
                scroll_item: true,
                text: true,
                legacy: true,
            },
            ..Default::default()
        },
        children: vec![FakeNode {
            properties: Properties {
                name: Some("child".into()),
                ..Default::default()
            },
            ..Default::default()
        }],
    }));
    if std::env::var("GCU_TEST_FAULT").as_deref() == Ok("normal_unknown") {
        source.reply(
            Pattern::Invoke,
            Err(gcu_core::driver::DriverError::Native {
                message: "synthetic uncertain provider failure".into(),
                maybe_executed: true,
            }),
        );
    }
    let driver = WindowsDriver::with_uia(api, source, Arc::new(FakeUiaEvents::default()))
        .await
        .unwrap();
    // Ensure discovery is ready before the server's first probe.
    driver
        .resolve_app(&AppQuery("Editor".into()))
        .await
        .unwrap();
    let wgc = Arc::new(FakeCapture::new(CapturePath::Wgc));
    wgc.paint(
        8,
        Paint {
            own: [20, 30, 40, 255],
            cover: [255, 0, 0, 255],
            occluded: true,
            mark: None,
        },
    );
    let print = Arc::new(FakeCapture::new(CapturePath::PrintWindow));
    Arc::new(driver.with_capture(Arc::new(CaptureBackend::new(wgc, print))))
}
