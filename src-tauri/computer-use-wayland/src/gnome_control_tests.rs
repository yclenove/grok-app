use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};
use zbus::zvariant::Value;

struct Extensions {
    state: Arc<AtomicU32>,
    setting: Arc<AtomicBool>,
    enabled: bool,
    discovered: bool,
    version: &'static str,
    accept: bool,
    apply: bool,
    calls: Arc<AtomicU32>,
    helper_alive: Arc<AtomicBool>,
}
#[zbus::interface(name = "org.gnome.Shell.Extensions")]
impl Extensions {
    #[zbus(property)]
    fn shell_version(&self) -> &str {
        self.version
    }
    #[zbus(property)]
    fn user_extensions_enabled(&self) -> bool {
        self.enabled
    }
    fn get_extension_info(&self, uuid: &str) -> HashMap<String, OwnedValue> {
        assert_eq!(uuid, GNOME_HELPER_UUID);
        if !self.discovered {
            return HashMap::new();
        }
        HashMap::from([
            (
                "enabled".into(),
                OwnedValue::from(self.setting.load(Ordering::SeqCst)),
            ),
            ("uuid".into(), Value::from(uuid).try_to_owned().unwrap()),
            (
                "path".into(),
                Value::from(
                    "/home/test/.local/share/gnome-shell/extensions/computer-use@grok-app.local",
                )
                .try_to_owned()
                .unwrap(),
            ),
            ("type".into(), OwnedValue::from(2.0f64)),
            (
                "state".into(),
                OwnedValue::from(self.state.load(Ordering::SeqCst) as f64),
            ),
        ])
    }
    fn enable_extension(&self, uuid: &str) -> bool {
        assert_eq!(uuid, GNOME_HELPER_UUID);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.accept && self.apply {
            self.setting.store(true, Ordering::SeqCst);
            self.state.store(1, Ordering::SeqCst);
            self.helper_alive.store(true, Ordering::SeqCst);
        }
        self.accept
    }
    fn disable_extension(&self, uuid: &str) -> bool {
        assert_eq!(uuid, GNOME_HELPER_UUID);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.accept && self.apply {
            self.setting.store(false, Ordering::SeqCst);
            if self.state.load(Ordering::SeqCst) != 3 {
                self.state.store(2, Ordering::SeqCst);
            }
            self.helper_alive.store(false, Ordering::SeqCst);
        }
        self.accept
    }
}
struct Helper(Arc<AtomicBool>);
#[zbus::interface(name = "org.grok.ComputerUse.NativePolicy1")]
impl Helper {
    fn get_state(&self) -> zbus::fdo::Result<(u32, String, u32, bool)> {
        if !self.0.load(Ordering::SeqCst) {
            return Err(zbus::fdo::Error::UnknownObject("disabled".into()));
        }
        Ok((1, "b35fd080-b9b4-4d74-8aeb-3269565ed550".into(), 0, false))
    }
}
struct Fixture {
    _bus: crate::tests::Bus,
    service: Connection,
    control: GnomeHelperControl,
    calls: Arc<AtomicU32>,
}
async fn fixture(
    enabled: bool,
    discovered: bool,
    version: &'static str,
    accept: bool,
    apply: bool,
) -> Fixture {
    let bus = crate::tests::Bus::start();
    let calls = Arc::new(AtomicU32::new(0));
    let alive = Arc::new(AtomicBool::new(false));
    let service = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(SHELL)
        .unwrap()
        .serve_at(
            PATH,
            Extensions {
                state: Arc::new(AtomicU32::new(2)),
                setting: Arc::new(AtomicBool::new(false)),
                enabled,
                discovered,
                version,
                accept,
                apply,
                calls: calls.clone(),
                helper_alive: alive.clone(),
            },
        )
        .unwrap()
        .serve_at(HELPER_PATH, Helper(alive))
        .unwrap()
        .build()
        .await
        .unwrap();
    let connection = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let control = GnomeHelperControl::on_connection(connection).await.unwrap();
    Fixture {
        _bus: bus,
        service,
        control,
        calls,
    }
}

#[tokio::test]
async fn fixed_shell_endpoint_round_trips_real_dbus_enable_and_disable() {
    let f = fixture(true, true, "46.0", true, true).await;
    let snapshot = f.control.snapshot().await.unwrap();
    assert!(snapshot.supported());
    assert!(snapshot.inactive());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0, "status is read only");
    assert_eq!(f.control.bus_id.len(), 32);
    f.control.set_enabled(true).await.unwrap();
    assert!(f.control.snapshot().await.unwrap().active());
    f.control.set_enabled(false).await.unwrap();
    assert!(f.control.snapshot().await.unwrap().inactive());
    assert_eq!(f.calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn settings_acceptance_does_not_prove_helper_activation() {
    let f = fixture(true, true, "46.0", true, false).await;
    let error = f.control.set_enabled(true).await.unwrap_err();
    assert!(error.contains("unconfirmed"));
    assert_eq!(
        f.calls.load(Ordering::SeqCst),
        1,
        "never auto replay a mutation"
    );
}
#[tokio::test]
async fn unsupported_missing_and_global_disabled_do_not_send_enable() {
    for (enabled, discovered, version) in [
        (true, true, "47.0"),
        (true, false, "46.0"),
        (false, true, "46.0"),
    ] {
        let f = fixture(enabled, discovered, version, true, true).await;
        assert!(f.control.set_enabled(true).await.is_err());
        assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test]
async fn shell_refusal_and_owner_loss_are_not_success() {
    let f = fixture(true, true, "46.0", false, true).await;
    assert!(f.control.set_enabled(true).await.is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    f.service.release_name(SHELL).await.unwrap();
    assert!(f.control.snapshot().await.is_err());
    assert!(f.control.set_enabled(false).await.is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn strict_gnome46_variant_schema_rejects_malformed_and_foreign_metadata() {
    let fixture = Extensions {
        state: Arc::new(AtomicU32::new(2)),
        setting: Arc::new(AtomicBool::new(false)),
        enabled: true,
        discovered: true,
        version: "46.0",
        accept: true,
        apply: true,
        calls: Arc::new(AtomicU32::new(0)),
        helper_alive: Arc::new(AtomicBool::new(false)),
    };
    for value in [
        OwnedValue::from(f64::NAN),
        OwnedValue::from(-1.0f64),
        OwnedValue::from(1.5f64),
        OwnedValue::from(1234.0f64),
        OwnedValue::from(2u32),
    ] {
        let mut info = fixture.get_extension_info(GNOME_HELPER_UUID);
        info.insert("state".into(), value);
        assert!(parse_extension(info).is_err());
    }
    let mut info = fixture.get_extension_info(GNOME_HELPER_UUID);
    info.insert(
        "uuid".into(),
        Value::from("foreign").try_to_owned().unwrap(),
    );
    assert!(parse_extension(info).is_err());
    for value in [
        OwnedValue::from(0u32),
        Value::from("false").try_to_owned().unwrap(),
    ] {
        let mut info = fixture.get_extension_info(GNOME_HELPER_UUID);
        info.insert("enabled".into(), value);
        assert!(parse_extension(info).is_err());
    }
    let mut info = fixture.get_extension_info(GNOME_HELPER_UUID);
    info.remove("enabled");
    assert!(parse_extension(info).is_err());
    assert!(parse_extension(HashMap::new()).unwrap().is_none());
}

#[tokio::test]
async fn sticky_error_can_retire_only_after_enable_setting_is_false_and_helper_absent() {
    let f = fixture(true, true, "46.0", true, true).await;
    let interface = f
        .service
        .object_server()
        .interface::<_, Extensions>(PATH)
        .await
        .unwrap();
    {
        let ext = interface.get().await;
        ext.state.store(3, Ordering::SeqCst);
        ext.setting.store(true, Ordering::SeqCst);
    }
    let snapshot = f.control.snapshot().await.unwrap();
    assert!(!snapshot.inactive());
    assert!(snapshot.needs_restart());
    assert!(!snapshot.enableable());
    assert!(f.control.set_enabled(true).await.is_err());
    assert_eq!(f.calls.load(Ordering::SeqCst), 0);
    f.control.set_enabled(false).await.unwrap();
    let snapshot = f.control.snapshot().await.unwrap();
    assert_eq!(
        snapshot.extension.as_ref().unwrap().state,
        3,
        "GNOME preserves ERROR after disable"
    );
    assert!(snapshot.inactive());
    assert!(!snapshot.enableable());
    assert!(snapshot.needs_restart());
    interface
        .get()
        .await
        .helper_alive
        .store(true, Ordering::SeqCst);
    assert!(
        !f.control.snapshot().await.unwrap().inactive(),
        "disabled setting never substitutes for retiring the helper endpoint"
    );
}

#[test]
fn terminal_errors_are_repairable_not_enableable_and_transitions_never_retire() {
    for state in 1..=8 {
        for enabled in [false, true] {
            let mut snapshot = HelperSnapshot {
                shell_version: "46.0".into(),
                user_extensions_enabled: true,
                extension: Some(HelperExtension {
                    state,
                    enabled,
                    path: "/tmp/owned".into(),
                    per_user: true,
                }),
                health: HelperHealth::Absent,
            };
            assert_eq!(
                snapshot.inactive(),
                !enabled && matches!(state, 2 | 3 | 4 | 6)
            );
            assert_eq!(snapshot.enableable(), !enabled && matches!(state, 2 | 6));
            for health in [
                HelperHealth::Ready,
                HelperHealth::Blocked,
                HelperHealth::Unknown,
            ] {
                snapshot.health = health;
                assert!(!snapshot.inactive());
                assert!(!snapshot.enableable());
            }
        }
    }
}
