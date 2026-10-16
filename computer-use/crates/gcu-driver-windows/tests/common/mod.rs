//! Shared portable fixture helpers; no native OS calls.
#![allow(dead_code)]
use gcu_core::{
    driver::*,
    model::{NativeHandle, NodeAction, TextRange, WindowPoint},
};
use gcu_driver_windows::{
    api::Shortcut,
    testing::{medium_security, signed_image, window, FakeWinApi},
    WindowsDriver, WindowsElement,
};
use std::{sync::Arc, time::Duration};

#[derive(Default)]
pub struct TestStop(pub std::sync::atomic::AtomicBool);
impl gcu_driver_windows::input::hotkey::StopLatch for TestStop {
    fn is_stopped(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn request_stop(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
pub fn fake_guard() -> Arc<gcu_driver_windows::input::hotkey::SafetyGuard> {
    gcu_driver_windows::input::hotkey::FakeSafetyApi::default()
        .start(Default::default(), Arc::new(TestStop::default()))
}

pub async fn fixture() -> (Arc<FakeWinApi>, WindowsDriver, AppRef, WindowRef) {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        s.paths.insert(7, "C:\\Editor.exe".into());
        s.images
            .insert("C:\\Editor.exe".into(), signed_image("C:\\Editor.exe"));
        s.processes.insert(7, medium_security());
        s.windows.push(window(8, 7));
        s.desktops.insert(8, Some(true));
    });
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef {
        app: app.clone(),
        id: info.id,
    };
    (api, driver, app, win)
}

pub fn actions(win: &WindowRef) -> Vec<DriverAction> {
    let el = NativeHandle::new(WindowsElement {
        window: win.clone(),
    });
    let mut actions = vec![
        DriverAction::Press { el: el.clone() },
        DriverAction::SecondaryAction {
            el: el.clone(),
            action: NodeAction::Press,
        },
        DriverAction::SetValue {
            el: el.clone(),
            value: "fixture".into(),
        },
        DriverAction::SelectText {
            el: el.clone(),
            range: TextRange {
                start: 0,
                length: 1,
            },
        },
        DriverAction::ScrollElement {
            el,
            dir: Dir::Down,
            pages: 1.0,
        },
        DriverAction::Activate { win: win.clone() },
    ];
    for route in [
        PointerRoute::Auto,
        PointerRoute::TargetedOnly,
        PointerRoute::AllowForeground,
    ] {
        actions.push(DriverAction::PointerClick {
            win: win.clone(),
            at: WindowPoint { x: 1.0, y: 1.0 },
            button: Button::Left,
            count: 1,
            route,
        });
        actions.push(DriverAction::PointerScroll {
            win: win.clone(),
            at: WindowPoint { x: 1.0, y: 1.0 },
            dx: 0,
            dy: 1,
            route,
        });
        actions.push(DriverAction::PointerDrag {
            win: win.clone(),
            path: vec![WindowPoint { x: 1.0, y: 1.0 }],
            route,
        });
    }
    for route in [
        KeyRoute::Auto,
        KeyRoute::TargetedOnly,
        KeyRoute::AllowForeground,
    ] {
        actions.push(DriverAction::TypeText {
            win: win.clone(),
            focus: None,
            text: "fixture".into(),
            route,
        });
        actions.push(DriverAction::PressKeys {
            win: win.clone(),
            chord: KeyChord::parse("ctrl+a").unwrap(),
            route,
        });
    }
    actions
}

pub fn shortcut() -> Shortcut {
    Shortcut {
        link: "C:\\StartMenu\\Editor.lnk".into(),
        name: "Editor".into(),
        target: "C:\\Editor.exe".into(),
        aumid: None,
    }
}

pub async fn indexed(api: &Arc<FakeWinApi>, driver: &WindowsDriver) {
    for _ in 0..100 {
        if !driver.discovery_status().indexing {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("index did not finish: {:?}", api.state().unwrap());
}
