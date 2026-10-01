//! Original private bus/name lifetimes; no access to a desktop session.
use super::*;

async fn churn(shell: &Shell, prefix: &str) {
    for n in 0..96 {
        let name = format!("{prefix}.Unrelated{n}");
        shell.server.request_name(name.as_str()).await.unwrap();
        shell.server.release_name(name.as_str()).await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrelated_name_churn_cannot_backpressure_the_original_monitor_connection() {
    let login = Fixture::new(Snapshot::default()).await;
    let shell = Shell::new(false, false, false).await;
    let watch = shell.watch(&login).await.unwrap();
    let policy = watch.input_policy();
    let (connection, owner) = watch.native_endpoint();
    // The watch is deliberately retained, not polled yet. A subscription to
    // every service fills its 64-message queue and stalls unrelated replies on
    // this same connection. All churn is real bus-authored NameOwnerChanged.
    churn(&shell, "org.grok").await;
    let reply = tokio::time::timeout(Duration::from_secs(1), async {
        zbus::Proxy::new(&connection, owner.as_str(), SHIELD_PATH, IFACE)
            .await
            .unwrap()
            .call::<_, _, bool>("GetActive", &())
            .await
    })
    .await;
    // Always retire the retained original subscriptions before asserting.
    let (stop, stopped) = tokio::sync::oneshot::channel();
    stop.send(()).unwrap();
    watch.run_until(stopped).await.unwrap();
    assert!(!policy.input_available());
    assert!(
        matches!(reply, Ok(Ok(false))),
        "unrelated name churn blocked the original connection: {reply:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_name_prefixes_do_not_match_but_each_exact_original_name_still_retires() {
    for name in [SHELL, SHIELD] {
        let login = Fixture::new(Snapshot::default()).await;
        let shell = Shell::new(false, false, false).await;
        let watch = shell.watch(&login).await.unwrap();
        let policy = watch.input_policy();
        churn(&shell, name).await;
        let (_stop, stopped) = tokio::sync::oneshot::channel();
        let monitor = tokio::spawn(watch.run_until(stopped));
        ready(&policy).await;
        assert!(!monitor.is_finished());
        shell.server.release_name(name).await.unwrap();
        assert!(ended(monitor).await.contains("owner lost or replaced"));
        shell.server.request_name(name).await.unwrap();
        assert!(!policy.input_available());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unpolled_original_name_release_reacquire_is_terminal_despite_identical_readback() {
    for name in [SHELL, SHIELD] {
        let login = Fixture::new(Snapshot::default()).await;
        let shell = Shell::new(false, false, false).await;
        let watch = shell.watch(&login).await.unwrap();
        let policy = watch.input_policy();
        let original = shell.server.unique_name().unwrap().to_string();
        shell.server.release_name(name).await.unwrap();
        shell.server.request_name(name).await.unwrap();
        let dbus = zbus::fdo::DBusProxy::new(&shell.server).await.unwrap();
        assert_eq!(
            dbus.get_name_owner(name.try_into().unwrap())
                .await
                .unwrap()
                .as_str(),
            original
        );
        // Fence the retained client through the same bus after the real ABA.
        let (connection, _) = watch.native_endpoint();
        zbus::fdo::DBusProxy::new(&connection)
            .await
            .unwrap()
            .get_id()
            .await
            .unwrap();
        let (_stop, stopped) = tokio::sync::oneshot::channel();
        let error = tokio::time::timeout(Duration::from_secs(2), watch.run_until(stopped))
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.contains("owner lost or replaced"), "{error}");
        assert!(!policy.input_available());
    }
}

struct OwnedBusPath(std::path::PathBuf);
impl OwnedBusPath {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let path =
            std::env::temp_dir().join(format!("grok-cu-bus-{}", uuid::Uuid::new_v4().simple()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    fn address(&self) -> String {
        format!("unix:path={}", self.0.join("bus").display())
    }
    fn remove_stopped_socket(&self) {
        use std::os::unix::fs::FileTypeExt;
        let path = self.0.join("bus");
        match path.symlink_metadata() {
            Ok(metadata) => {
                assert!(metadata.file_type().is_socket());
                std::fs::remove_file(path).unwrap();
            }
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        }
    }
}
impl Drop for OwnedBusPath {
    fn drop(&mut self) {
        self.remove_stopped_socket();
        std::fs::remove_dir(&self.0).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacement_bus_same_socket_and_unique_owner_never_rearms_original_policy() {
    let path = OwnedBusPath::new();
    let login = Fixture::new(Snapshot::default()).await;
    let mut first = Shell::on_bus(Bus::start_on(Some(&path.address())), false, false, false).await;
    let original_pid = first.bus.child.id();
    let original_name = first.server.unique_name().unwrap().to_string();
    let original_id = zbus::fdo::DBusProxy::new(&first.server)
        .await
        .unwrap()
        .get_id()
        .await
        .unwrap()
        .to_string();
    let login_watch = login.watch().await.unwrap();
    let login_policy = login_watch.input_policy();
    let watch = GnomeSessionWatch::prepare(login_watch, first.client().await)
        .await
        .unwrap();
    let policy = watch.input_policy();
    let (original_connection, _) = watch.native_endpoint();
    let (_stop, stopped) = tokio::sync::oneshot::channel();
    let monitor = tokio::spawn(watch.run_until(stopped));
    ready(&policy).await;
    first.bus.child.kill().unwrap();
    let original_exit = first.bus.child.wait().unwrap();
    assert!(!original_exit.success());
    ended(monitor).await;
    assert!(!policy.input_available());
    assert!(!login_policy.input_available());
    path.remove_stopped_socket();
    let mut second = Shell::on_bus(Bus::start_on(Some(&path.address())), false, false, false).await;
    let replacement_id = zbus::fdo::DBusProxy::new(&second.server)
        .await
        .unwrap()
        .get_id()
        .await
        .unwrap()
        .to_string();
    assert_eq!(
        second.server.unique_name().unwrap().as_str(),
        original_name,
        "fixture must exercise real unique-name reuse"
    );
    assert_ne!(original_id, replacement_id, "bus identity must change");
    assert_ne!(original_pid, second.bus.child.id());
    assert!(
        tokio::time::timeout(Duration::from_secs(1), async {
            zbus::fdo::DBusProxy::new(&original_connection)
                .await?
                .get_id()
                .await
                .map_err(zbus::Error::from)
        })
        .await
        .unwrap()
        .is_err(),
        "old connection silently reconnected"
    );
    let fresh = second.watch(&login).await.unwrap();
    let fresh_policy = fresh.input_policy();
    assert!(!fresh_policy.input_available());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let new_monitor = tokio::spawn(fresh.run_until(stopped));
    ready(&fresh_policy).await;
    // New test watch can become ready; old guards cannot. This is not App consent.
    assert!(!policy.input_available());
    assert!(!login_policy.input_available());
    stop.send(()).unwrap();
    new_monitor.await.unwrap().unwrap();
    assert!(!fresh_policy.input_available());
    second.bus.child.kill().unwrap();
    second.bus.child.wait().unwrap();
    assert!(first.bus.child.try_wait().unwrap().is_some());
    assert!(second.bus.child.try_wait().unwrap().is_some());
}
