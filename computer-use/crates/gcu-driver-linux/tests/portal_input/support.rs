#![allow(dead_code)]
use async_trait::async_trait;
use gcu_core::driver::{AppRef, WindowRef};
pub use gcu_core::{
    driver::{Cancel, DriverAction, DriverError, KeyRoute, PointerRoute},
    model::WindowPoint,
};
pub use gcu_driver_linux::wayland::{
    ei::{EiSnapshot, EiTransport, InputEvent, InputRegion},
    input::{InputConfig, PortalInput},
    input_control::{
        AnchoredClock, InputAuthority, RemoteDesktopConnection, RemoteDesktopPortal, StopControl,
        WATCHDOG_TICK,
    },
    restore::{RestoreRecord, RestoreTokenStore},
    windows::{StreamIdentity, WindowBinding},
};
use gcu_driver_linux::{
    wayland::{
        build_environment_capabilities,
        portal::{PortalError, PortalScreenCastSession, PortalSourceKind, PortalStream},
        windows::WindowRecord,
        Compositor, LinuxEnvironment, LinuxSessionType, PortalInfo, TargetEnvironment,
    },
    x11::hotkey::SharedStopLatch,
    AppIdentity,
};
pub use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::{collections::BTreeMap, future::Future};
use tokio::time::Instant;

pub async fn drive<F: Future>(future: F) -> F::Output {
    // Keep the paused reactor runnable while off-reactor token IO completes. No wall-time sleep.
    tokio::pin!(future);
    loop {
        tokio::select! {biased;result=&mut future=>return result,_=tokio::task::yield_now()=>{}}
    }
}
type EventHook = Arc<dyn Fn(&InputEvent) + Send + Sync>;

#[derive(Default)]
pub struct FakeEi {
    pub log: Mutex<Vec<(Instant, InputEvent)>>,
    pub revision: AtomicUsize,
    pub disconnected: AtomicBool,
    pub fail_at: AtomicUsize,
    pub calls: AtomicUsize,
    pub fail_ups: AtomicBool,
    pub hook: Mutex<Option<EventHook>>,
    pub mapping: Mutex<Option<String>>,
}
impl FakeEi {
    pub fn events(&self) -> Vec<InputEvent> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(_, e)| e.clone())
            .collect()
    }
}
impl EiTransport for FakeEi {
    fn snapshot(&self) -> Option<EiSnapshot> {
        if self.disconnected.load(Ordering::SeqCst) {
            return None;
        }
        let keys = BTreeMap::from([
            (0xffe1, (42, false)),
            (0xffe3, (29, false)),
            (0xffe9, (56, false)),
            (0xffeb, (125, false)),
            (u32::from('a'), (30, false)),
            (u32::from('A'), (30, true)),
        ]);
        Some(EiSnapshot {
            generation: self.revision.load(Ordering::SeqCst) as u64 + 1,
            region: InputRegion {
                mapping_id: self
                    .mapping
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "m1".into()),
                x: 100,
                y: 200,
                width: 800,
                height: 600,
            },
            keys,
            text: true,
        })
    }
    fn send(&self, event: &InputEvent, generation: Option<u64>) -> Result<(), DriverError> {
        if self.disconnected.load(Ordering::SeqCst)
            || generation.is_some_and(|g| g != self.revision.load(Ordering::SeqCst) as u64 + 1)
        {
            return Err(DriverError::CapabilityUnavailable);
        }
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        self.log
            .lock()
            .unwrap()
            .push((Instant::now(), event.clone()));
        let hook = self.hook.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(event);
        }
        if self.fail_at.load(Ordering::SeqCst) == n
            || self.fail_ups.load(Ordering::SeqCst)
                && matches!(
                    event,
                    InputEvent::Key { pressed: false, .. }
                        | InputEvent::Button { pressed: false, .. }
                )
        {
            return Err(DriverError::Native {
                message: "injected EI failure".into(),
                maybe_executed: true,
            });
        }
        Ok(())
    }
    fn disconnect(&self) {
        self.disconnected.store(true, Ordering::SeqCst);
    }
}
#[derive(Debug)]
struct FakeSession {
    active: Arc<AtomicBool>,
    closes: Arc<AtomicUsize>,
}
#[async_trait]
impl PortalScreenCastSession for FakeSession {
    async fn close(&self) -> Result<(), PortalError> {
        self.active.store(false, Ordering::SeqCst);
        self.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
    fn handle(&self) -> &str {
        "/session/1"
    }
}
pub struct FakePortal {
    pub deny: AtomicBool,
    pub tokens: Mutex<Vec<Option<String>>>,
    pub active: Arc<AtomicBool>,
    pub closes: Arc<AtomicUsize>,
    pub stream: Mutex<PortalStream>,
    ei: Arc<FakeEi>,
}
#[async_trait]
impl RemoteDesktopPortal for FakePortal {
    async fn open(&self, restore: Option<&str>) -> Result<RemoteDesktopConnection, DriverError> {
        self.tokens.lock().unwrap().push(restore.map(str::to_owned));
        if self.deny.load(Ordering::SeqCst) {
            return Err(DriverError::PermissionMissing {
                permission: "remote_desktop".into(),
            });
        }
        self.active.store(true, Ordering::SeqCst);
        self.ei.disconnected.store(false, Ordering::SeqCst);
        Ok(RemoteDesktopConnection {
            session: Arc::new(FakeSession {
                active: self.active.clone(),
                closes: self.closes.clone(),
            }),
            stream: self.stream.lock().unwrap().clone(),
            restore_token: Some("rotated".into()),
            ei: self.ei.clone(),
        })
    }
}
pub struct Authority {
    pub confined: AtomicBool,
    pub fail: AtomicBool,
    pub pending: AtomicBool,
    pub mismatch: AtomicBool,
    pub calls: AtomicUsize,
    pub entered: Cancel,
}
#[async_trait]
impl InputAuthority for Authority {
    async fn prove(
        &self,
        session: &str,
        stream: &PortalStream,
        window: &WindowBinding,
    ) -> Result<StreamIdentity, DriverError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.cancel();
        if self.pending.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(DriverError::CapabilityUnavailable);
        }
        Ok(StreamIdentity {
            session: session.into(),
            node_id: stream.node_id,
            serial: if self.mismatch.load(Ordering::SeqCst) {
                2
            } else {
                1
            },
            source: "exact-source".into(),
            window: window.clone(),
        })
    }
    fn confined(&self) -> bool {
        self.confined.load(Ordering::SeqCst)
    }
}
pub struct Harness {
    pub tmp: tempfile::TempDir,
    pub store: Arc<RestoreTokenStore>,
    pub ei: Arc<FakeEi>,
    pub portal: FakePortal,
    pub authority: Arc<Authority>,
    pub stop: Arc<StopControl>,
    pub latch: Arc<SharedStopLatch>,
    pub binding: WindowBinding,
    pub win: WindowRef,
}
impl Harness {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(RestoreTokenStore::new(tmp.path().join("t")));
        let ei = Arc::new(FakeEi::default());
        let portal = FakePortal {
            deny: AtomicBool::new(false),
            tokens: Mutex::new(vec![]),
            active: Arc::new(AtomicBool::new(false)),
            closes: Arc::new(AtomicUsize::new(0)),
            stream: Mutex::new(PortalStream {
                node_id: 42,
                source_type: PortalSourceKind::Window,
                position: None,
                size: Some((800, 600)),
                mapping_id: Some("m1".into()),
                pipewire_serial: Some(1),
            }),
            ei: ei.clone(),
        };
        let authority = Arc::new(Authority {
            confined: AtomicBool::new(true),
            fail: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            mismatch: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            entered: Cancel::default(),
        });
        let latch = Arc::new(SharedStopLatch::default());
        let stop = StopControl::registered(latch.clone());
        let binding = WindowBinding {
            compositor: Compositor::Gnome,
            epoch: "owner-1".into(),
            record: WindowRecord {
                id: "wayland-7".into(),
                generation: 1,
                title: "Fixture".into(),
                app_id: "fixture".into(),
                pid: Some(123),
                peer_pid: true,
            },
            image: AppIdentity {
                exe_path: "/usr/bin/fixture".into(),
                dev: 1,
                inode: 42,
                sha256: "ab".repeat(32),
                pid: 123,
                start_time: 456,
                desktop_id: None,
            },
        };
        let win = WindowRef {
            id: binding.record.id.clone(),
            app: AppRef {
                info: gcu_protocol::AppInfo {
                    id: "fixture".into(),
                    name: "Fixture".into(),
                    pid: Some(123),
                    identity: None,
                },
            },
        };
        Self {
            tmp,
            store,
            ei,
            portal,
            authority,
            stop,
            latch,
            binding,
            win,
        }
    }
    pub fn config(&self) -> InputConfig {
        let env = LinuxEnvironment {
            session_type: LinuxSessionType::WaylandPure,
            compositor: Compositor::Gnome,
            desktop_name: None,
            wayland_display: Some("wayland-0".into()),
            x11_display: None,
        };
        let portal = PortalInfo {
            available: true,
            screencast_version: Some(4),
            remote_desktop_version: Some(2),
            global_shortcuts_version: Some(1),
        };
        InputConfig {
            window: self.win.clone(),
            binding: self.binding.clone(),
            capabilities: build_environment_capabilities(
                &env,
                TargetEnvironment::NativeWayland,
                &portal,
                false,
                false,
                false,
                false,
                false,
            ),
            tokens: self.store.clone(),
            token_key: "s1".into(),
            auth_version: "v1".into(),
            auth_expires_secs: 2000,
            clock: Arc::new(AnchoredClock::new(1000)),
            physical_size: (1600, 1200),
        }
    }
    pub async fn open(&self) -> Result<Arc<PortalInput>, DriverError> {
        drive(PortalInput::open(
            self.config(),
            &self.portal,
            self.authority.clone(),
            self.stop.clone(),
            &Cancel::default(),
        ))
        .await
    }
    pub fn click(&self) -> DriverAction {
        DriverAction::PointerClick {
            win: self.win.clone(),
            at: WindowPoint { x: 800., y: 600. },
            button: gcu_core::driver::Button::Left,
            count: 1,
            route: PointerRoute::AllowForeground,
        }
    }
    pub fn keys(&self) -> DriverAction {
        DriverAction::PressKeys {
            win: self.win.clone(),
            chord: gcu_protocol::KeyChord::parse("ctrl+shift+a").unwrap(),
            route: KeyRoute::AllowForeground,
        }
    }
}
