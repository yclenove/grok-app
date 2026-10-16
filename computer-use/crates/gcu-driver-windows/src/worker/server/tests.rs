//! In-memory worker regressions; no subprocess startup or wall-clock sleeps.
use super::*;
use crate::{
    testing::{medium_security, signed_image, window, ControlledBlock, FakeWinApi},
    WindowsDriver,
};

async fn fixture() -> (Arc<FakeWinApi>, WindowsDriver, Arc<ControlledBlock>) {
    let api = Arc::new(FakeWinApi::default());
    api.update(|s| {
        // Catalog stops before it can verify the target or refresh again.
        for (pid, path) in [(9, "C:\\Slow.exe"), (7, "C:\\gcu-fixture.exe")] {
            s.paths.insert(pid, path.into());
            s.images.insert(path.into(), signed_image(path));
            s.processes.insert(pid, medium_security());
            s.windows.push(window(u64::from(pid), pid));
        }
    });
    let blocked = api.block_image("C:\\Slow.exe");
    let driver = WindowsDriver::with_api(api.clone()).await.unwrap();
    blocked.wait_entered().await;
    (api, driver, blocked)
}

fn target_reads(api: &FakeWinApi) -> usize {
    api.state()
        .unwrap()
        .image_reads
        .iter()
        .filter(|path| *path == "c:\\gcu-fixture.exe")
        .count()
}

async fn resolve(driver: &dyn Driver, handles: &mut Handles) -> gcu_protocol::AppInfo {
    let Reply::App(app) = dispatch(driver, handles, 1, Request::ResolveApp("fixture".into()))
        .await
        .unwrap()
    else {
        panic!("expected exact fixture app")
    };
    assert_eq!(app.pid, Some(7));
    app
}

#[tokio::test]
async fn list_windows_bound_app_verifies_once_and_enumerates_once() {
    let (api, driver, blocked) = fixture().await;
    let mut handles = Handles::default();
    let initial_reads = target_reads(&api);
    let app = resolve(&driver, &mut handles).await;
    assert_eq!(
        target_reads(&api) - initial_reads,
        1,
        "initial resolution requires provenance"
    );
    let before = api.state().unwrap().enumerations;
    let Reply::Windows(windows) = dispatch(
        &driver,
        &mut handles,
        1,
        Request::ListWindows(Some(app.clone())),
    )
    .await
    .unwrap() else {
        panic!("expected windows")
    };
    assert_eq!(windows.len(), 1);
    assert_eq!(api.state().unwrap().enumerations - before, 1);
    assert_eq!(
        target_reads(&api) - initial_reads,
        2,
        "one necessary verification, no re-resolution"
    );
    // Caching the binding must not cache away authorization revalidation.
    api.update(|s| s.images.get_mut("C:\\gcu-fixture.exe").unwrap().sha256 = "f".repeat(64));
    assert!(matches!(
        dispatch(&driver, &mut handles, 1, Request::ListWindows(Some(app))).await,
        Err(DriverError::AppNotFound)
    ));
    blocked.release();
}

#[tokio::test]
async fn key_window_bound_app_verifies_image_once_without_rehashing_selected_hwnd() {
    let (api, driver, blocked) = fixture().await;
    let mut handles = Handles::default();
    let app = resolve(&driver, &mut handles).await;
    let before = target_reads(&api);
    assert!(matches!(
        dispatch(&driver, &mut handles, 1, Request::KeyWindow(app)).await,
        Ok(Reply::Window(_))
    ));
    assert_eq!(target_reads(&api) - before, 1);
    blocked.release();
}

#[tokio::test]
async fn list_windows_restarted_worker_rehydrates_exact_process_without_duplicate_enumeration() {
    let (api, driver, blocked) = fixture().await;
    let app = resolve(&driver, &mut Handles::default()).await;
    let mut restarted = Handles::default();
    let before = api.state().unwrap().enumerations;
    assert!(matches!(
        dispatch(
            &driver,
            &mut restarted,
            2,
            Request::ListWindows(Some(app.clone()))
        )
        .await,
        Ok(Reply::Windows(_))
    ));
    // One live resolution plus the requested Win32 enumeration, not a third pass.
    assert_eq!(api.state().unwrap().enumerations - before, 2);
    let mut wrong_pid = app;
    wrong_pid.pid = Some(77);
    assert!(matches!(
        dispatch(
            &driver,
            &mut restarted,
            2,
            Request::ListWindows(Some(wrong_pid))
        )
        .await,
        Err(DriverError::AppNotFound)
    ));
    blocked.release();
}

#[tokio::test]
async fn handshake_pending_initializer_never_advertises_unwarmed_capabilities() {
    let (api, driver, blocked) = fixture().await;
    let (mut parent, child) = tokio::io::duplex(8192);
    let (reader, writer) = tokio::io::split(child);
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, gate) = tokio::sync::oneshot::channel();
    let server = serve(reader, writer, |_| async move {
        entered.send(()).unwrap();
        gate.await.unwrap(); // Models controlled UIA initialization, not elapsed time.
        Ok(Arc::new(driver) as Arc<dyn Driver>)
    });
    tokio::pin!(server);
    codec::write_frame(
        &mut parent,
        &Call {
            version: VERSION,
            id: 0,
            epoch: 1,
            namespace: "1.1".into(),
            request: Request::Doctor,
        },
    )
    .await
    .unwrap();
    tokio::select! {
        result = &mut server => panic!("server completed before initialization: {result:?}"),
        _ = waiting => (),
    }
    assert_eq!(
        api.state().unwrap().enumerations,
        1,
        "only background catalog has run"
    );
    release.send(()).unwrap();
    tokio::select! {
        result = &mut server => panic!("server completed before handshake: {result:?}"),
        response = codec::read_frame::<_, Response>(&mut parent) => {
            assert!(matches!(response.unwrap().unwrap().result, Ok(Reply::Doctor { ready: true, .. })));
        }
    }
    drop(parent);
    server.await.unwrap();
    blocked.release();
}
