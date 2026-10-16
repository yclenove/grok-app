//! Deterministic identity/TOCTOU tests; no desktop, account or wall-clock sleeps.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use async_trait::async_trait;
use gcu_core::driver::Cancel;
use gcu_driver_linux::{
    identity::AppIdentity,
    wayland::{
        windows::{
            IdentityError, ProcessSource, StreamIdentity, WaylandWindows, WindowBackend,
            WindowRecord, WindowSnapshot,
        },
        Compositor,
    },
};
use std::sync::{Arc, Mutex};

#[path = "wayland_identity/gnome.rs"]
mod gnome;

fn image() -> AppIdentity {
    AppIdentity {
        exe_path: "/usr/bin/fixture".into(),
        dev: 1,
        inode: 42,
        sha256: "ab".repeat(32),
        pid: 123,
        start_time: 456,
        desktop_id: None,
    }
}

#[derive(Clone)]
struct Processes(Arc<Mutex<AppIdentity>>);
#[async_trait]
impl ProcessSource for Processes {
    async fn inspect(&self, pid: u32) -> Result<AppIdentity, IdentityError> {
        let image = self.0.lock().unwrap().clone();
        if pid != image.pid {
            return Err(IdentityError::Handoff);
        }
        Ok(image)
    }
}

#[derive(Clone)]
struct Backend {
    state: Arc<Mutex<WindowSnapshot>>,
    activations: Arc<Mutex<u32>>,
    change_on_read: Arc<Mutex<Option<WindowSnapshot>>>,
}
#[async_trait]
impl WindowBackend for Backend {
    fn compositor(&self) -> Compositor {
        self.state.lock().unwrap().compositor
    }
    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError> {
        let before = self.state.lock().unwrap().clone();
        if let Some(after) = self.change_on_read.lock().unwrap().take() {
            *self.state.lock().unwrap() = after;
        }
        Ok(before)
    }
    async fn activate(
        &self,
        expected: &WindowSnapshot,
        window: &WindowRecord,
        _image: &AppIdentity,
    ) -> Result<(), IdentityError> {
        let actual = self.state.lock().unwrap();
        if actual.epoch != expected.epoch || !actual.windows.contains(window) {
            return Err(IdentityError::StaleWindow);
        }
        *self.activations.lock().unwrap() += 1;
        Ok(())
    }
}

fn setup() -> (WaylandWindows, Backend, Processes) {
    let backend = Backend {
        state: Arc::new(Mutex::new(WindowSnapshot {
            compositor: Compositor::Gnome,
            epoch: "shell-owner:extension-generation".into(),
            windows: vec![WindowRecord {
                id: "7".into(),
                generation: 1,
                title: "Fixture".into(),
                app_id: "org.fixture".into(),
                pid: Some(123),
                peer_pid: true,
            }],
        })),
        activations: Arc::new(Mutex::new(0)),
        change_on_read: Arc::new(Mutex::new(None)),
    };
    let processes = Processes(Arc::new(Mutex::new(image())));
    let windows = WaylandWindows::new(Arc::new(backend.clone()), Arc::new(processes.clone()));
    (windows, backend, processes)
}

#[tokio::test]
async fn bind_forged_window_id_or_pid_rejects_without_activation() {
    let (windows, backend, _) = setup();
    assert_eq!(
        windows
            .bind("8", &image(), &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::WindowGone
    );
    backend.state.lock().unwrap().windows[0].pid = Some(999);
    assert_eq!(
        windows
            .bind("7", &image(), &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Handoff
    );
    assert_eq!(*backend.activations.lock().unwrap(), 0);
}

#[tokio::test]
async fn bind_client_supplied_pid_or_missing_pid_observation_only() {
    for pid in [Some(123), None] {
        let (windows, backend, _) = setup();
        backend.state.lock().unwrap().windows[0].peer_pid = false;
        backend.state.lock().unwrap().windows[0].pid = pid;
        assert_eq!(
            windows
                .list(&Cancel::default())
                .await
                .unwrap()
                .windows
                .len(),
            1
        );
        assert_eq!(
            windows
                .bind("7", &image(), &Cancel::default())
                .await
                .unwrap_err(),
            IdentityError::Handoff
        );
    }
}

#[tokio::test]
async fn bind_similar_or_exact_title_never_substitutes_window() {
    let (windows, backend, _) = setup();
    for title in ["Fixture", "Fixture — document"] {
        backend.state.lock().unwrap().windows[0].title = title.into();
        assert_eq!(
            windows
                .bind(title, &image(), &Cancel::default())
                .await
                .unwrap_err(),
            IdentityError::WindowGone
        );
    }
}

#[tokio::test]
async fn revalidate_reused_window_or_reloaded_extension_rejects() {
    for reload in [false, true] {
        let (windows, backend, _) = setup();
        let binding = windows
            .bind("7", &image(), &Cancel::default())
            .await
            .unwrap();
        if reload {
            backend.state.lock().unwrap().epoch = "new-owner".into();
        } else {
            backend.state.lock().unwrap().windows[0].generation += 1;
        }
        assert_eq!(
            windows
                .revalidate(&binding, &Cancel::default())
                .await
                .unwrap_err(),
            IdentityError::StaleWindow
        );
    }
}

#[tokio::test]
async fn revalidate_recycled_pid_or_updated_image_requires_reauthorization() {
    for field in 0..6 {
        let (windows, backend, processes) = setup();
        let binding = windows
            .bind("7", &image(), &Cancel::default())
            .await
            .unwrap();
        {
            let mut image = processes.0.lock().unwrap();
            match field {
                0 => image.exe_path = "/usr/bin/replaced".into(),
                1 => image.dev += 1,
                2 => image.inode += 1,
                3 => image.sha256 = "cd".repeat(32), // same path/dev/inode: in-place update
                4 => image.start_time += 1,
                _ => image.pid += 1,
            }
        }
        assert!(windows
            .activate(&binding, true, &Cancel::default())
            .await
            .is_err());
        assert_eq!(*backend.activations.lock().unwrap(), 0);
    }
}

#[tokio::test]
async fn activate_policy_denial_or_cancel_prevents_dispatch() {
    let (windows, backend, _) = setup();
    let binding = windows
        .bind("7", &image(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        windows
            .activate(&binding, false, &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Unsupported
    );
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(
        windows.activate(&binding, true, &cancel).await.unwrap_err(),
        IdentityError::Cancelled {
            maybe_executed: false
        }
    );
    assert_eq!(*backend.activations.lock().unwrap(), 0);
}

#[tokio::test]
async fn activate_window_reuse_after_read_compositor_fence_rejects() {
    let (windows, backend, _) = setup();
    let binding = windows
        .bind("7", &image(), &Cancel::default())
        .await
        .unwrap();
    let mut next = backend.state.lock().unwrap().clone();
    next.windows[0].generation += 1;
    *backend.change_on_read.lock().unwrap() = Some(next);
    assert_eq!(
        windows
            .activate(&binding, true, &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::StaleWindow
    );
    assert_eq!(*backend.activations.lock().unwrap(), 0);
}

#[tokio::test]
async fn revalidate_title_or_desktop_id_change_keeps_exact_identity() {
    let (windows, backend, processes) = setup();
    let binding = windows
        .bind("7", &image(), &Cancel::default())
        .await
        .unwrap();
    backend.state.lock().unwrap().windows[0].title = "Renamed document".into();
    processes.0.lock().unwrap().desktop_id = Some("changed.desktop".into());
    windows
        .revalidate(&binding, &Cancel::default())
        .await
        .unwrap();
    windows
        .activate(&binding, true, &Cancel::default())
        .await
        .unwrap();
    assert_eq!(*backend.activations.lock().unwrap(), 1);
}

#[tokio::test]
async fn stream_different_window_process_or_source_rejects() {
    let (windows, _, _) = setup();
    let binding = windows
        .bind("7", &image(), &Cancel::default())
        .await
        .unwrap();
    let stream = StreamIdentity {
        session: "portal-session".into(),
        node_id: 17,
        serial: 25,
        source: "window-source".into(),
        window: binding.clone(),
    };
    windows
        .check_stream(&binding, &stream, &stream, &Cancel::default())
        .await
        .unwrap();
    for field in 0..7 {
        let mut wrong = stream.clone();
        match field {
            0 => wrong.session = "other-session".into(),
            1 => wrong.node_id += 1,
            2 => wrong.serial += 1,
            3 => wrong.source = "monitor".into(),
            4 => wrong.window.record.id = "8".into(),
            5 => wrong.window.image.pid += 1,
            _ => wrong.window.epoch = "other-shell".into(),
        }
        assert_eq!(
            windows
                .check_stream(&binding, &stream, &wrong, &Cancel::default())
                .await
                .unwrap_err(),
            IdentityError::StreamMismatch
        );
    }
}

#[tokio::test]
async fn list_unknown_compositor_explicitly_rejects_capability() {
    let (windows, backend, _) = setup();
    for compositor in [Compositor::Unknown, Compositor::Other] {
        backend.state.lock().unwrap().compositor = compositor;
        assert_eq!(
            windows.list(&Cancel::default()).await.unwrap_err(),
            IdentityError::Unsupported
        );
    }
}

#[tokio::test]
async fn bind_duplicate_window_ids_rejects_ambiguous_source() {
    let (windows, backend, _) = setup();
    let duplicate = backend.state.lock().unwrap().windows[0].clone();
    backend.state.lock().unwrap().windows.push(duplicate);
    assert_eq!(
        windows
            .bind("7", &image(), &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Handoff
    );
}

#[tokio::test]
async fn revalidate_in_place_image_change_returns_reauthorize() {
    let (windows, backend, processes) = setup();
    let binding = windows
        .bind("7", &image(), &Cancel::default())
        .await
        .unwrap();
    processes.0.lock().unwrap().sha256 = "cd".repeat(32);
    assert_eq!(
        windows
            .activate(&binding, true, &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Reauthorize
    );
    assert_eq!(*backend.activations.lock().unwrap(), 0);
}

struct HungBackend {
    snapshot: WindowSnapshot,
    hang_read: bool,
}
#[async_trait]
impl WindowBackend for HungBackend {
    fn compositor(&self) -> Compositor {
        Compositor::Gnome
    }
    async fn snapshot(&self) -> Result<WindowSnapshot, IdentityError> {
        if self.hang_read {
            std::future::pending().await
        } else {
            Ok(self.snapshot.clone())
        }
    }
    async fn activate(
        &self,
        _: &WindowSnapshot,
        _: &WindowRecord,
        _: &AppIdentity,
    ) -> Result<(), IdentityError> {
        std::future::pending().await
    }
}

#[tokio::test(start_paused = true)]
async fn native_timeout_injected_clock_distinguishes_read_and_delivery() {
    for hang_read in [true, false] {
        let (_, backend, processes) = setup();
        let backend = HungBackend {
            snapshot: backend.state.lock().unwrap().clone(),
            hang_read,
        };
        let windows = WaylandWindows::new(Arc::new(backend), Arc::new(processes));
        if hang_read {
            assert_eq!(
                windows.list(&Cancel::default()).await.unwrap_err(),
                IdentityError::Timeout {
                    maybe_executed: false
                }
            );
        } else {
            let binding = windows
                .bind("7", &image(), &Cancel::default())
                .await
                .unwrap();
            assert_eq!(
                windows
                    .activate(&binding, true, &Cancel::default())
                    .await
                    .unwrap_err(),
                IdentityError::Timeout {
                    maybe_executed: true
                }
            );
        }
    }
}

#[tokio::test]
async fn foreign_toplevel_supplied_pid_remains_observation_only() {
    use gcu_driver_linux::wayland::toplevel::{ForeignToplevelSource, ToplevelBackend};
    struct Source(WindowSnapshot);
    #[async_trait]
    impl ForeignToplevelSource for Source {
        async fn inventory(&self) -> Result<WindowSnapshot, IdentityError> {
            Ok(self.0.clone())
        }
    }
    let (_, backend, processes) = setup();
    let mut snapshot = backend.state.lock().unwrap().clone();
    snapshot.compositor = Compositor::Wlroots;
    let windows = WaylandWindows::new(
        Arc::new(ToplevelBackend::new(Arc::new(Source(snapshot)))),
        Arc::new(processes),
    );
    let inventory = windows.list(&Cancel::default()).await.unwrap();
    assert_eq!(inventory.windows[0].pid, None);
    assert!(!inventory.windows[0].peer_pid);
    assert_eq!(
        windows
            .bind("7", &image(), &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Handoff
    );
}

#[test]
fn kwin_spoofed_sender_and_stale_reports_fail_closed() {
    use gcu_driver_linux::wayland::kwin::{KwinReports, ReportClock};
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Clock(AtomicU64);
    impl ReportClock for Clock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::Relaxed)
        }
    }
    let clock = Arc::new(Clock(AtomicU64::new(100)));
    let reports = KwinReports::new(":1.42".into(), clock.clone()).unwrap();
    let records = vec![(
        "uuid".into(),
        1,
        "Fixture".into(),
        "org.fixture".into(),
        123,
        false,
    )];
    assert_eq!(
        reports
            .receive(":1.99", "epoch".into(), records.clone())
            .unwrap_err(),
        IdentityError::Handoff
    );
    assert!(reports.latest().is_err());
    reports
        .receive(":1.42", "epoch".into(), records.clone())
        .unwrap();
    assert!(!reports.latest().unwrap().windows[0].peer_pid);
    let mut forged = records;
    forged[0].5 = true;
    assert_eq!(
        reports
            .receive(":1.42", "epoch".into(), forged)
            .unwrap_err(),
        IdentityError::Handoff
    );
    clock.0.store(2100, Ordering::Relaxed);
    assert_eq!(reports.latest().unwrap_err(), IdentityError::Handoff);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn process_inspection_native_binds_open_image_path_device_inode_digest() {
    use gcu_driver_linux::wayland::windows::NativeProcesses;
    use std::os::unix::fs::MetadataExt;
    let image = NativeProcesses.inspect(std::process::id()).await.unwrap();
    let path = std::fs::canonicalize("/proc/self/exe").unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    assert_eq!(image.exe_path, path);
    assert_eq!(image.dev, metadata.dev());
    assert_eq!(image.inode, metadata.ino());
    assert_eq!(
        image.sha256,
        gcu_driver_linux::identity::compute_sha256(&path).unwrap()
    );
    assert!(image.start_time > 0);
    assert_eq!(
        NativeProcesses.inspect(u32::MAX).await.unwrap_err(),
        IdentityError::Handoff
    );
}

#[test]
fn compositor_scripts_syntax_and_security_fences_pass() {
    // Syntax and mocked policy tests run on all hosted CI targets. This is not a
    // claim that GNOME/KWin native desktop behavior was exercised.
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/check-scripts.mjs");
    let result = std::process::Command::new("node")
        .arg(script)
        .output()
        .expect("Node is required for compositor script static checks");
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
