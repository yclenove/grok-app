//! Exercise the real GNOME adapter through a fake D-Bus boundary on every OS.
use super::*;
use gcu_driver_linux::wayland::gnome::{CompositorBus, CompositorPeer, GnomeBackend, WireWindow};
use std::collections::HashMap;

#[derive(Clone)]
struct Images(Arc<Mutex<HashMap<u32, AppIdentity>>>);
#[async_trait]
impl ProcessSource for Images {
    async fn inspect(&self, pid: u32) -> Result<AppIdentity, IdentityError> {
        self.0
            .lock()
            .unwrap()
            .get(&pid)
            .cloned()
            .ok_or(IdentityError::Handoff)
    }
}
struct State {
    owner: String,
    pid: u32,
    epoch: String,
    records: Vec<WireWindow>,
    calls: u32,
    lost_reply: bool,
    change_image_on_list: bool,
    change_owner_on_list: bool,
}
struct Bus {
    state: Mutex<State>,
    images: Images,
}
#[async_trait]
impl CompositorBus for Bus {
    async fn owner(&self, _: &str) -> Result<String, IdentityError> {
        Ok(self.state.lock().unwrap().owner.clone())
    }
    async fn pid(&self, _: &str) -> Result<u32, IdentityError> {
        Ok(self.state.lock().unwrap().pid)
    }
    async fn list(&self, _: &str) -> Result<(u32, String, Vec<WireWindow>), IdentityError> {
        let mut state = self.state.lock().unwrap();
        if state.change_image_on_list {
            self.images.0.lock().unwrap().get_mut(&123).unwrap().sha256 = "cd".repeat(32);
        }
        if state.change_owner_on_list {
            state.owner = ":1.99".into();
        }
        Ok((1, state.epoch.clone(), state.records.clone()))
    }
    async fn activate(
        &self,
        owner: &str,
        epoch: &str,
        window: &WindowRecord,
    ) -> Result<bool, IdentityError> {
        let mut state = self.state.lock().unwrap();
        if state.owner != owner
            || state.epoch != epoch
            || !state.records.iter().any(|r| {
                r.0 == window.id && r.1 == window.generation && Some(r.4) == window.pid && r.5
            })
        {
            return Ok(false);
        }
        state.calls += 1;
        if state.lost_reply {
            Err(IdentityError::Transport {
                maybe_executed: true,
            })
        } else {
            Ok(true)
        }
    }
}

fn fixture() -> (Arc<Bus>, AppIdentity) {
    let mut shell = image();
    shell.pid = 900;
    shell.exe_path = "/usr/bin/gnome-shell".into();
    let images = Images(Arc::new(Mutex::new(HashMap::from([
        (900, shell.clone()),
        (123, image()),
    ]))));
    (
        Arc::new(Bus {
            images,
            state: Mutex::new(State {
                owner: ":1.42".into(),
                pid: 900,
                epoch: "extension-epoch".into(),
                records: vec![(
                    "7".into(),
                    1,
                    "Fixture".into(),
                    "org.fixture".into(),
                    123,
                    true,
                )],
                calls: 0,
                lost_reply: false,
                change_image_on_list: false,
                change_owner_on_list: false,
            }),
        }),
        shell,
    )
}

async fn backend(bus: Arc<Bus>, shell: AppIdentity) -> GnomeBackend {
    let peer = CompositorPeer::pin(
        bus.clone(),
        Compositor::Gnome,
        shell,
        Arc::new(bus.images.clone()),
    )
    .await
    .unwrap();
    GnomeBackend::new(peer).unwrap()
}

#[tokio::test]
async fn compositor_forged_bus_pid_or_image_fails_pin() {
    let (bus, shell) = fixture();
    bus.state.lock().unwrap().pid = 123;
    assert!(matches!(
        CompositorPeer::pin(
            bus.clone(),
            Compositor::Gnome,
            shell,
            Arc::new(bus.images.clone())
        )
        .await,
        Err(IdentityError::Handoff)
    ));
    let (bus, mut shell) = fixture();
    shell.sha256 = "cd".repeat(32);
    assert!(matches!(
        CompositorPeer::pin(
            bus.clone(),
            Compositor::Gnome,
            shell,
            Arc::new(bus.images.clone())
        )
        .await,
        Err(IdentityError::Reauthorize)
    ));
}

#[tokio::test]
async fn compositor_owner_replaced_before_or_during_list_rejects() {
    for during in [false, true] {
        let (bus, shell) = fixture();
        let backend = backend(bus.clone(), shell).await;
        if during {
            bus.state.lock().unwrap().change_owner_on_list = true;
        } else {
            bus.state.lock().unwrap().owner = ":1.99".into();
        }
        assert_eq!(
            backend.snapshot().await.unwrap_err(),
            IdentityError::StaleWindow
        );
    }
}

#[tokio::test]
async fn activate_image_updated_during_compositor_read_requires_reauthorization() {
    let (bus, shell) = fixture();
    let backend = backend(bus.clone(), shell).await;
    let snapshot = backend.snapshot().await.unwrap();
    bus.state.lock().unwrap().change_image_on_list = true;
    assert_eq!(
        backend
            .activate(&snapshot, &snapshot.windows[0], &image())
            .await
            .unwrap_err(),
        IdentityError::Reauthorize
    );
    assert_eq!(bus.state.lock().unwrap().calls, 0);
}

#[tokio::test]
async fn activate_lost_dbus_reply_reports_unknown_without_replay() {
    let (bus, shell) = fixture();
    let windows = WaylandWindows::new(
        Arc::new(backend(bus.clone(), shell).await),
        Arc::new(bus.images.clone()),
    );
    let binding = windows
        .bind("7", &image(), &Cancel::default())
        .await
        .unwrap();
    bus.state.lock().unwrap().lost_reply = true;
    assert_eq!(
        windows
            .activate(&binding, true, &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Transport {
            maybe_executed: true
        }
    );
    assert_eq!(bus.state.lock().unwrap().calls, 1);
}

#[tokio::test]
async fn compositor_duplicate_ids_and_xwayland_pid_cannot_bind() {
    let (bus, shell) = fixture();
    let windows = WaylandWindows::new(
        Arc::new(backend(bus.clone(), shell).await),
        Arc::new(bus.images.clone()),
    );
    bus.state.lock().unwrap().records[0].5 = false;
    assert_eq!(
        windows
            .bind("7", &image(), &Cancel::default())
            .await
            .unwrap_err(),
        IdentityError::Handoff
    );
    let duplicate = bus.state.lock().unwrap().records[0].clone();
    bus.state.lock().unwrap().records.push(duplicate);
    assert_eq!(
        windows.list(&Cancel::default()).await.unwrap_err(),
        IdentityError::Handoff
    );
}
