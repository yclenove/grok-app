//! Owned compositor/Xvfb acceptance only. Never use an ambient user display.
use super::*;
use grok_computer_use_core::{
    broker::{BrokerOptions, ComputerUseBroker},
    fake::FakeAdapter,
    session_grants::SessionGrants,
};
use std::time::Instant;

fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let context = glib::MainContext::default();
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "native parent condition timed out"
        );
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn window() -> gtk::Window {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Owned Computer Use parent acceptance");
    window.set_default_size(240, 120);
    window.show_all();
    until(|| window.is_mapped());
    window
}
fn ready(lease: &ParentLease) -> String {
    until(|| !matches!(lease.state(), ParentState::Pending));
    lease.handle().unwrap().unwrap()
}
fn closed(lease: &ParentLease) {
    until(|| lease.state() == ParentState::Closed);
}
fn begin(grants: &SessionGrants, broker: &ComputerUseBroker, name: &str) -> AuthorizationTicket {
    grants.begin(broker, name, None).unwrap()
}
fn marker(name: &str) {
    eprintln!("PARENT_CASE {name}");
}

pub fn run() -> serde_json::Value {
    let fixture = std::env::var("GROK_CU_PARENT_FIXTURE").expect("owned native fixture required");
    assert!(matches!(fixture.as_str(), "owned-labwc" | "owned-xvfb"));
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap();
    assert!(runtime.starts_with("/var/tmp/grok-cu-parent-runtime."));
    if fixture == "owned-labwc" {
        assert!(std::env::var("DISPLAY").is_err());
        assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("wayland"));
    } else {
        assert_eq!(std::env::var("GDK_BACKEND").as_deref(), Ok("x11"));
    }
    gtk::init().unwrap();
    fn sendable<T: Send + Sync>() {}
    sendable::<ParentLease>();
    let broker = ComputerUseBroker::new(
        Arc::new(FakeAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::path::Path::new(&runtime).join("parent.lease"),
            ..BrokerOptions::default()
        },
    );
    let grants = SessionGrants::default();
    let mut passed = Vec::new();

    marker("popup-rejected-before-native-export");
    let popup = gtk::Window::new(gtk::WindowType::Popup);
    let t = begin(&grants, &broker, "popup");
    let error = match export(&popup, &t) {
        Err(error) => error,
        Ok(_) => panic!("popup exported as a native parent"),
    };
    assert!(error.contains("toplevel"));
    unsafe {
        popup.destroy();
    }
    passed.push("popup-rejected-before-native-export");

    marker("unmapped-rejected");
    let hidden = gtk::Window::new(gtk::WindowType::Toplevel);
    let t = begin(&grants, &broker, "hidden-initial");
    assert!(export(&hidden, &t).is_err());
    unsafe {
        hidden.destroy();
    }
    passed.push("unmapped-rejected");

    marker("cancel-before-export");
    let w = window();
    let t = begin(&grants, &broker, "cancel-before");
    grants.fence_fail_checked(&broker, &t, || {});
    assert!(export(&w, &t).is_err());
    passed.push("cancel-before-export");
    if fixture == "owned-xvfb" {
        marker("x11-no-fallback");
        let t = begin(&grants, &broker, "x11-negative");
        let error = match export(&w, &t) {
            Err(e) => e,
            Ok(_) => panic!("X11 parent accepted as Wayland"),
        };
        assert!(error.contains("not Wayland"));
        unsafe {
            w.destroy();
        }
        passed.push("x11-no-fallback");
        marker("complete");
        return serde_json::json!({"fixture":fixture,"passed":passed,"nativeExports":0,"installedApp":false,"actualGnome":false});
    }
    unsafe {
        w.destroy();
    }

    marker("reference-count-begin");
    let w = window();
    let ta = begin(&grants, &broker, "reference-a");
    let tb = begin(&grants, &broker, "reference-b");
    let a = export(&w, &ta).unwrap();
    let b = export(&w, &tb).unwrap();
    let ha = ready(&a);
    let hb = ready(&b);
    assert!(ha.starts_with("wayland:") && ha.len() > 8);
    assert_eq!(ha, hb);
    grants.publish(&broker, &ta, true, || {}).unwrap();
    assert!(
        export(&w, &ta).is_err(),
        "completed ticket restarted export"
    );
    marker("reference-close-first");
    a.request_close();
    closed(&a);
    assert_eq!(b.handle().unwrap(), Some(hb));
    marker("reference-close-last");
    b.request_close();
    closed(&b);
    assert!(a.handle().is_err());
    unsafe {
        w.destroy();
    }
    passed.push("same-window-reference-count-and-completed-ticket");

    marker("cancel-after-ready");
    let w = window();
    let t = begin(&grants, &broker, "cancel-ready");
    let lease = export(&w, &t).unwrap();
    let new_handle = ready(&lease);
    assert_ne!(new_handle, ha);
    grants.fence_fail_checked(&broker, &t, || {});
    assert!(lease.handle().is_err());
    closed(&lease);
    unsafe {
        w.destroy();
    }
    passed.push("cancel-after-ready");

    marker("cancel-before-native-callback");
    let w = window();
    let t = begin(&grants, &broker, "cancel-callback");
    let lease = export(&w, &t).unwrap();
    assert_eq!(lease.state(), ParentState::Pending);
    grants.fence_fail_checked(&broker, &t, || {});
    assert!(lease.handle().is_err());
    closed(&lease);
    assert!(lease.handle().is_err());
    unsafe {
        w.destroy();
    }
    passed.push("cancel-before-native-callback");

    marker("lease-drop-original-owner");
    let w = window();
    let t = begin(&grants, &broker, "drop-lease");
    let lease = export(&w, &t).unwrap();
    ready(&lease);
    let observer = lease.shared.clone();
    drop(lease);
    until(|| *observer.state.borrow() == ParentState::Closed);
    unsafe {
        w.destroy();
    }
    passed.push("lease-drop-original-owner");

    marker("parent-destroyed");
    let w = window();
    let t = begin(&grants, &broker, "destroyed-parent");
    let lease = export(&w, &t).unwrap();
    ready(&lease);
    unsafe {
        w.destroy();
    }
    assert!(lease.handle().is_err());
    closed(&lease);
    passed.push("parent-destroyed");

    marker("shared-parent-destroyed");
    let w = window();
    let ta = begin(&grants, &broker, "destroy-shared-a");
    let tb = begin(&grants, &broker, "destroy-shared-b");
    let a = export(&w, &ta).unwrap();
    let b = export(&w, &tb).unwrap();
    assert_eq!(ready(&a), ready(&b));
    unsafe {
        w.destroy();
    }
    assert!(a.handle().is_err());
    assert!(b.handle().is_err());
    closed(&a);
    closed(&b);
    passed.push("shared-parent-destroyed-exact-references");

    marker("parent-unmapped");
    let w = window();
    let t = begin(&grants, &broker, "unmapped-parent");
    let lease = export(&w, &t).unwrap();
    ready(&lease);
    w.hide();
    assert!(lease.handle().is_err());
    closed(&lease);
    w.show_all();
    until(|| w.is_mapped());
    assert!(lease.handle().is_err(), "remap revived old parent export");
    unsafe {
        w.destroy();
    }
    passed.push("parent-unmapped-no-revival");

    marker("deadline-before-native-callback");
    let w = window();
    let t = begin(&grants, &broker, "deadline");
    let lease = export(&w, &t).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(rt.block_on(lease.ready(Duration::ZERO)).is_err());
    assert!(lease.handle().is_err());
    closed(&lease);
    unsafe {
        w.destroy();
    }
    passed.push("deadline-before-native-callback");

    marker("worker-ready-and-close");
    let w = window();
    let t = begin(&grants, &broker, "worker");
    let lease = export(&w, &t).unwrap();
    let worker = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            assert!(lease
                .ready(Duration::from_secs(3))
                .await
                .unwrap()
                .starts_with("wayland:"));
            lease.request_close();
            tokio::time::timeout(Duration::from_secs(3), lease.closed())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(lease.state(), ParentState::Closed);
        });
    });
    until(|| worker.is_finished());
    worker.join().unwrap();
    unsafe {
        w.destroy();
    }
    passed.push("worker-ready-and-close");
    marker("complete");
    serde_json::json!({"fixture":fixture,"passed":passed,"nativeExports":9,"exportCalls":11,
        "installedApp":false,"actualGnome":false,"portalConsentWired":false})
}
