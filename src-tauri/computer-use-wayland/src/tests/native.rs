//! Actual private PipeWire daemon + C video source + real SCM_RIGHTS portal.
//! This is NOT a native GNOME/desktop acceptance claim.
#[path = "native_host.rs"]
mod host;
use super::*;
use serde_json::Value as Json;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Clone)]
pub(super) struct NativeGrant {
    pub socket: PathBuf,
    pub node: u32,
    pub serial: u64,
}
struct Process(Child);

// This guard can only refer to a child spawned and retained by this fixture.
// Always resume on unwind; never signal a desktop or a process found by name.
struct PausedDaemon<'a>(&'a Process);
impl Drop for PausedDaemon<'_> {
    fn drop(&mut self) {
        unsafe { libc::kill(self.0 .0.id() as libc::pid_t, libc::SIGCONT) };
    }
}
async fn pause_daemon(daemon: &Process) -> PausedDaemon<'_> {
    let pid = daemon.0.id();
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) }, 0);
    let guard = PausedDaemon(daemon);
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        if status.lines().any(|line| line.starts_with("State:\tT")) {
            return guard;
        }
        assert!(Instant::now() < deadline, "owned daemon did not stop");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Native {
    root: PathBuf,
    remote: String,
    source: Process,
    daemon: Process,
    grant: NativeGrant,
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.source.0.kill();
        let _ = self.source.0.wait();
        let _ = self.daemon.0.kill();
        let _ = self.daemon.0.wait();
        // Keep logs for evidence, never clean another run or the user session.
        eprintln!("owned PipeWire logs: {}", self.root.display());
    }
}
fn tool(name: &str) -> PathBuf {
    std::env::var_os("GROK_CU_PW_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin"))
        .join(name)
}
fn configure(command: &mut Command, root: &Path, remote: &str) {
    owned_child(command);
    command
        .env("XDG_RUNTIME_DIR", root)
        .env("PIPEWIRE_RUNTIME_DIR", root)
        .env("PIPEWIRE_REMOTE", remote)
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY");
}
fn query(root: &Path, remote: &str) -> Json {
    let mut command = Command::new("timeout");
    command.args(["--kill-after=1s", "4s"]).arg(tool("pw-dump"));
    configure(&mut command, root, remote);
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "pw-dump: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn property<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    value.get("info")?.get("props")?.get(key)
}
fn number(value: &Json) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
impl Native {
    async fn start() -> Self {
        let source_exe = std::env::var_os("GROK_CU_PW_SOURCE")
            .expect("build tests/pw-source.c and set GROK_CU_PW_SOURCE; not skippable");
        let root =
            std::env::temp_dir().join(format!("grok-cu-pw-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let remote = "owned-cu-pipewire".to_string();
        let config = root.join("owned.conf");
        fs::write(
            &config,
            r#"
context.properties = { core.daemon = true core.name = owned-cu-pipewire }
context.spa-libs = { support.* = support/libspa-support }
context.modules = [
 { name = libpipewire-module-protocol-native }
 { name = libpipewire-module-client-node }
 { name = libpipewire-module-link-factory }
 { name = libpipewire-module-metadata }
 { name = libpipewire-module-access args = { access.force = unrestricted } }
]
"#,
        )
        .unwrap();
        let mut command = Command::new(tool("pipewire"));
        command
            .arg("-c")
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(fs::File::create(root.join("daemon.stdout")).unwrap())
            .stderr(fs::File::create(root.join("daemon.stderr")).unwrap());
        configure(&mut command, &root, &remote);
        let mut daemon = Process(command.spawn().unwrap());
        let socket = root.join(&remote);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() {
            assert!(
                daemon.0.try_wait().unwrap().is_none(),
                "owned daemon died: {}",
                fs::read_to_string(root.join("daemon.stderr")).unwrap()
            );
            assert!(Instant::now() < deadline, "owned daemon socket deadline");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut command = Command::new(source_exe);
        configure(&mut command, &root, &remote);
        command
            .stdin(Stdio::piped())
            .stdout(fs::File::create(root.join("source.stdout")).unwrap())
            .stderr(fs::File::create(root.join("source.stderr")).unwrap());
        let mut source = Process(command.spawn().unwrap());
        let grant = loop {
            let snapshot = query(&root, &remote);
            if let Some(node) = snapshot.as_array().unwrap().iter().find(|obj| {
                property(obj, "node.name").and_then(Json::as_str) == Some("owned-cu-source")
            }) {
                break NativeGrant {
                    socket,
                    node: node["id"].as_u64().unwrap() as u32,
                    serial: number(property(node, "object.serial").unwrap()).unwrap(),
                };
            }
            assert!(
                source.0.try_wait().unwrap().is_none(),
                "source died: {}",
                fs::read_to_string(root.join("source.stderr")).unwrap()
            );
            assert!(Instant::now() < deadline, "source registry deadline");
            tokio::time::sleep(Duration::from_millis(30)).await;
        };
        Self {
            root,
            remote,
            source,
            daemon,
            grant,
        }
    }
    async fn link(&self, exclude: Option<u64>) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = query(&self.root, &self.remote);
            let objects = snapshot.as_array().unwrap();
            let sink = objects.iter().find(|v| {
                property(v, "node.name")
                    .and_then(Json::as_str)
                    .is_some_and(|s| s.starts_with("grok-cu-capture-"))
                    && v["id"].as_u64() != exclude
            });
            if let Some(sink) = sink {
                assert_eq!(
                    number(property(sink, "target.object").unwrap()),
                    Some(self.grant.serial)
                );
                let sink_id = sink["id"].as_u64().unwrap();
                let port = |node: u64| {
                    objects.iter().find(|v| {
                        v["type"] == "PipeWire:Interface:Port"
                            && property(v, "node.id").and_then(number) == Some(node)
                    })
                };
                if let (Some(output), Some(input)) = (port(self.grant.node as u64), port(sink_id)) {
                    let mut command = Command::new("timeout");
                    command
                        .args(["--kill-after=1s", "4s"])
                        .arg(tool("pw-link"))
                        .arg(output["id"].to_string())
                        .arg(input["id"].to_string());
                    configure(&mut command, &self.root, &self.remote);
                    let result = command.output().unwrap();
                    assert!(
                        result.status.success(),
                        "private link failed: {}",
                        String::from_utf8_lossy(&result.stderr)
                    );
                    return sink_id;
                }
            }
            assert!(
                Instant::now() < deadline,
                "capture port deadline; snapshot={snapshot}"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    fn no_capture_node(&self) {
        let snapshot = query(&self.root, &self.remote);
        if snapshot.as_array().unwrap().iter().any(|v| {
            property(v, "node.name")
                .and_then(Json::as_str)
                .is_some_and(|s| s.starts_with("grok-cu-capture-"))
        }) {
            // Diagnostic only: retain the original strict assertion below.
            // Do not turn a second successful read into a passing test.
            fs::write(
                self.root.join("retirement-first.json"),
                snapshot.to_string(),
            )
            .unwrap();
            std::thread::sleep(Duration::from_millis(100));
            let second = query(&self.root, &self.remote);
            fs::write(self.root.join("retirement-second.json"), second.to_string()).unwrap();
        }
        assert!(
            !snapshot
                .as_array()
                .unwrap()
                .iter()
                .any(|v| property(v, "node.name")
                    .and_then(Json::as_str)
                    .is_some_and(|s| s.starts_with("grok-cu-capture-"))),
            "capture node leaked after Closed"
        );
    }
    async fn source_command(&mut self, mode: u8) {
        let log = self.root.join("source.stdout");
        let previous = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .filter(|s| s.starts_with("MODE="))
            .count();
        self.source
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(&[mode])
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let content = fs::read_to_string(&log).unwrap();
            let modes: Vec<_> = content.lines().filter(|s| s.starts_with("MODE=")).collect();
            if modes.len() > previous {
                assert!(modes
                    .last()
                    .unwrap()
                    .starts_with(&format!("MODE={} ", char::from(mode))));
                break;
            }
            assert!(self.source.0.try_wait().unwrap().is_none());
            assert!(
                Instant::now() < deadline,
                "owned source command acknowledgement"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    fn assert_source_streaming(&mut self) {
        assert!(self.source.0.try_wait().unwrap().is_none());
        assert!(self.daemon.0.try_wait().unwrap().is_none());
        let log = fs::read_to_string(self.root.join("source.stdout")).unwrap();
        assert_eq!(
            log.lines().rfind(|s| s.starts_with("STATE=")),
            Some("STATE=streaming")
        );
        let snapshot = query(&self.root, &self.remote);
        let source = snapshot
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == self.grant.node)
            .unwrap();
        assert_eq!(source["info"]["state"], "running");
    }
}
async fn next_frame(session: &PortalSession, after: u64) -> CapturedFrame {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match session.frame() {
            Ok(Some(frame)) if frame.sequence > after => return frame,
            Err(message) => panic!("frame failed: {message}; {:?}", session.state()),
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "actual frame deadline: {:?}",
            session.state()
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires both owned native PipeWire and libeis fixtures"]
async fn actual_pixels_and_ei_share_portal_lifetime_and_capture_loss_releases_keys() {
    let mut native = Native::start().await;
    let eis = super::eis::NativeEis::start("owned-fixture-region").await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let mut session = PortalSession::on_native_test_bus(
        PortalOptions::new("joint-native-capture-input".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    super::eis::capabilities(&session).await;
    native.link(None).await;
    let frame = next_frame(&session, 0).await;
    let receipt = session
        .input_observed(
            session.observe().unwrap().unwrap(),
            InputAction::Key {
                code: 30,
                pressed: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.run_id, frame.run_id);
    eis.wait_event("key", 1).await;
    native.source.0.kill().unwrap();
    native.source.0.wait().unwrap();
    terminal(&mut session.subscribe()).await;
    let events = eis.wait_event("disconnect", 1).await;
    assert!(events
        .iter()
        .any(|e| e["event"] == "key" && e["code"] == 30 && e["pressed"] == false));
    assert!(session.frame().is_err());
    assert_eq!(session.input_state(), InputState::Revoked);
    session.stop().await.unwrap();
    native.no_capture_node();
    fixture.assert_sessions_closed();
    println!("PASS joint real PipeWire + libei owner: capture loss releases key and retires both consumers");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires both owned native PipeWire and libeis fixtures"]
async fn observed_native_input_binds_owner_geometry_transform_age_and_ei_generation() {
    let native = Native::start().await;
    let mut eis = super::eis::NativeEis::start("owned-fixture-region").await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let mut options = PortalOptions::new("observed-native".into(), SourceKind::Monitor);
    options.observation_timeout = Duration::from_secs(2);
    let mut session =
        PortalSession::on_native_test_bus(options, fixture.bus.address.clone()).unwrap();
    session.ready().await.unwrap();
    let caps = super::eis::capabilities(&session).await;
    native.link(None).await;
    next_frame(&session, 0).await;
    let bad_action = || InputAction::Key {
        code: 31,
        pressed: true,
    };

    // Equal user-visible run strings are not the identity of the native owners.
    let other_fixture = Fixture::new(Mode::Normal).await;
    let mut other = PortalSession::on_test_bus(
        PortalOptions::new("observed-native".into(), SourceKind::Monitor),
        other_fixture.bus.address.clone(),
    )
    .unwrap();
    other.ready().await.unwrap();
    assert!(other
        .input_observed(session.observe().unwrap().unwrap(), bad_action())
        .await
        .unwrap_err()
        .contains("another"));
    other.stop().await.unwrap();
    other_fixture.assert_sessions_closed();

    // No input has started emulating, so the libeis pause is a real transition.
    let old_generation = session.observe().unwrap().unwrap();
    eis.command(b'p');
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(session.input_state(),InputState::Ready(ref c) if !c.keyboard && c.generation>caps.generation)
        {
            break;
        }
        assert!(Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    eis.command(b'r');
    super::eis::capabilities(&session).await;
    assert!(session
        .input_observed(old_generation, bad_action())
        .await
        .unwrap_err()
        .contains("generation"));

    // Actual SPA metadata traverses PipeWire. Even an orientation ABA never
    // reauthorizes a ticket from before the change; all eight values are seen.
    let transforms = [
        crate::VideoTransform::Rotate90,
        crate::VideoTransform::Rotate180,
        crate::VideoTransform::Rotate270,
        crate::VideoTransform::Flipped,
        crate::VideoTransform::Flipped90,
        crate::VideoTransform::Flipped180,
        crate::VideoTransform::Flipped270,
        crate::VideoTransform::Normal,
    ];
    let before_aba = session.observe().unwrap().unwrap();
    for expected in transforms {
        let old = session.observe().unwrap().unwrap();
        let generation = old.frame().format_generation;
        assert_eq!(
            unsafe { libc::kill(native.source.0.id() as i32, libc::SIGUSR2) },
            0
        );
        let until = Instant::now() + Duration::from_secs(3);
        let mut seq = old.frame().sequence;
        loop {
            let frame = next_frame(&session, seq).await;
            seq = frame.sequence;
            if frame.transform == expected {
                assert_eq!(frame.format_generation, generation);
                break;
            }
            assert!(
                Instant::now() < until,
                "native transform metadata did not change"
            );
        }
        assert!(session
            .input_observed(old, bad_action())
            .await
            .unwrap_err()
            .contains("geometry changed"));
    }
    assert!(session
        .input_observed(before_aba, bad_action())
        .await
        .unwrap_err()
        .contains("geometry changed"));

    let old = session.observe().unwrap().unwrap();
    assert_eq!(
        unsafe { libc::kill(native.source.0.id() as i32, libc::SIGUSR1) },
        0
    );
    let until = Instant::now() + Duration::from_secs(3);
    let mut seq = old.frame().sequence;
    loop {
        let frame = next_frame(&session, seq).await;
        seq = frame.sequence;
        if frame.format_generation > old.frame().format_generation && frame.buffer_width == 48 {
            break;
        }
        assert!(Instant::now() < until, "native format did not change");
    }
    assert!(session
        .input_observed(old, bad_action())
        .await
        .unwrap_err()
        .contains("geometry changed"));

    let old = session.observe().unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    next_frame(&session, old.frame().sequence).await;
    assert!(session
        .input_observed(old, bad_action())
        .await
        .unwrap_err()
        .contains("expired"));
    assert!(matches!(session.input_state(), InputState::Ready(_)));
    assert!(!eis
        .events()
        .iter()
        .any(|e| e["event"] == "key" || e["event"] == "absolute"));

    let observation = session.observe().unwrap().unwrap();
    let run = observation.frame().run_id.clone();
    let receipt = session
        .input_observed(
            observation,
            InputAction::Key {
                code: 30,
                pressed: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(receipt.run_id, run);
    eis.wait_event("key", 1).await;
    session
        .input_observed(session.observe().unwrap().unwrap(), InputAction::ReleaseAll)
        .await
        .unwrap();
    session
        .input_observed(
            session.observe().unwrap().unwrap(),
            InputAction::Absolute { x: 100.0, y: 150.0 },
        )
        .await
        .unwrap();
    let events = eis.wait_event("absolute", 1).await;
    let motion = events.iter().find(|e| e["event"] == "absolute").unwrap();
    assert_eq!(motion["x"], 200.0);
    assert_eq!(motion["y"], 350.0);
    let closing = session.observe().unwrap().unwrap();
    session.stop().await.unwrap();
    assert!(session.input_observed(closing, bad_action()).await.is_err());
    let events = eis.wait_event("disconnect", 1).await;
    let keys: Vec<_> = events.iter().filter(|e| e["event"] == "key").collect();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|e| e["code"] == 30));
    assert_eq!(keys[0]["pressed"], true);
    assert_eq!(keys[1]["pressed"], false);
    native.no_capture_node();
    fixture.assert_sessions_closed();
    // Both descriptors have real servers here, so there are no fake socket
    // peers. EI disconnect and the absent PW capture node prove retirement.
    fixture.shared.assert_peers_closed(0);
    println!("PASS observed native input: owner + EI generation + eight transforms/ABA + resize + expiry + exact wire actions + Stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires both owned native PipeWire and libeis fixtures"]
async fn presented_pixels_map_eight_orientations_crop_resize_and_dpi_to_real_ei() {
    use crate::VideoTransform::*;
    let native = Native::start().await;
    let eis = super::eis::NativeEis::start("owned-fixture-region").await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let mut options = PortalOptions::new("presented-pixels".into(), SourceKind::Monitor);
    options.observation_timeout = Duration::from_secs(2);
    let mut session =
        PortalSession::on_native_test_bus(options, fixture.bus.address.clone()).unwrap();
    session.ready().await.unwrap();
    let caps = super::eis::capabilities(&session).await;
    assert_eq!(caps.absolute_region.unwrap().physical_scale, 2.0);
    native.link(None).await;
    next_frame(&session, 0).await;
    // Literal source-coordinate corner oracles. Metadata traverses a real PW
    // stream; bytes encode original x in blue and y in green, independently of
    // the renderer. The source crop is (2,2,28,20), not an EI desktop offset.
    let cases = [
        (Normal, 28, 20, [(2, 2), (29, 2), (2, 21), (29, 21)]),
        (Rotate90, 20, 28, [(2, 21), (2, 2), (29, 21), (29, 2)]),
        (Rotate180, 28, 20, [(29, 21), (2, 21), (29, 2), (2, 2)]),
        (Rotate270, 20, 28, [(29, 2), (29, 21), (2, 2), (2, 21)]),
        (Flipped, 28, 20, [(29, 2), (2, 2), (29, 21), (2, 21)]),
        (Flipped90, 20, 28, [(2, 2), (2, 21), (29, 2), (29, 21)]),
        (Flipped180, 28, 20, [(2, 21), (29, 21), (2, 2), (29, 2)]),
        (Flipped270, 20, 28, [(29, 21), (29, 2), (2, 21), (2, 2)]),
    ];
    let mut count = 0;
    for (index, (transform, w, h, corners)) in cases.into_iter().enumerate() {
        if index != 0 {
            let old = session
                .observe()
                .unwrap()
                .unwrap()
                .present(8192, 8192)
                .unwrap();
            assert_eq!(
                unsafe { libc::kill(native.source.0.id() as i32, libc::SIGUSR2) },
                0
            );
            let until = Instant::now() + Duration::from_secs(3);
            let mut seq = old.source_frame().sequence;
            loop {
                let frame = next_frame(&session, seq).await;
                seq = frame.sequence;
                if frame.transform == transform {
                    break;
                }
                assert!(Instant::now() < until);
            }
            assert!(session
                .move_to_pixel(old, 0, 0)
                .await
                .unwrap_err()
                .contains("geometry changed"));
        }
        let image = session
            .observe()
            .unwrap()
            .unwrap()
            .present(8192, 8192)
            .unwrap();
        assert_eq!(image.source_frame().transform, transform);
        assert_eq!((image.image().width(), image.image().height()), (w, h));
        for ((x, y), (sx, sy)) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)]
            .into_iter()
            .zip(corners)
        {
            let p = (y * w + x) as usize * 4;
            assert_eq!(
                &image.image().rgba()[p + 1..p + 4],
                &[sy, sx, 255],
                "{transform:?}"
            );
        }
        for (x, y) in [(w, 0), (0, h), (u32::MAX, u32::MAX)] {
            assert!(session
                .move_to_pixel(
                    session
                        .observe()
                        .unwrap()
                        .unwrap()
                        .present(8192, 8192)
                        .unwrap(),
                    x,
                    y
                )
                .await
                .unwrap_err()
                .contains("outside"));
        }
        assert_eq!(
            eis.events()
                .iter()
                .filter(|e| e["event"] == "absolute")
                .count(),
            count
        );
        for (x, y) in [(0, 0), (w - 1, h - 1)] {
            session
                .move_to_pixel(
                    session
                        .observe()
                        .unwrap()
                        .unwrap()
                        .present(8192, 8192)
                        .unwrap(),
                    x,
                    y,
                )
                .await
                .unwrap();
            count += 1;
            let events = eis.wait_event("absolute", count).await;
            let motion = events.iter().rfind(|e| e["event"] == "absolute").unwrap();
            // EIS region is (100,200,800,600). No second physical-scale factor.
            let expected_x = 100.0 + (f64::from(x) + 0.5) * 800.0 / f64::from(w);
            let expected_y = 200.0 + (f64::from(y) + 0.5) * 600.0 / f64::from(h);
            // Protocol fields are float32, then the C fixture prints 8 decimals.
            assert!((motion["x"].as_f64().unwrap() - f64::from(expected_x as f32)).abs() < 1e-8);
            assert!((motion["y"].as_f64().unwrap() - f64::from(expected_y as f32)).abs() < 1e-8);
        }
    }
    // Rotate metadata back to normal, then actually renegotiate source geometry.
    let old = session.observe().unwrap().unwrap().present(10, 10).unwrap();
    assert_eq!(
        unsafe { libc::kill(native.source.0.id() as i32, libc::SIGUSR2) },
        0
    );
    assert_eq!(
        unsafe { libc::kill(native.source.0.id() as i32, libc::SIGUSR1) },
        0
    );
    let until = Instant::now() + Duration::from_secs(4);
    let mut seq = old.source_frame().sequence;
    loop {
        let frame = next_frame(&session, seq).await;
        seq = frame.sequence;
        if frame.buffer_width == 48 && frame.transform == Normal {
            break;
        }
        assert!(Instant::now() < until);
    }
    assert!(session.move_to_pixel(old, 0, 0).await.is_err());
    let scaled = session.observe().unwrap().unwrap().present(11, 9).unwrap();
    assert_eq!((scaled.image().width(), scaled.image().height()), (11, 7));
    // Cropped 44x28 -> 11x7, centre samples at source (4,4) and (44,28).
    assert_eq!(&scaled.image().rgba()[1..4], &[4, 4, 255]);
    assert_eq!(
        &scaled.image().rgba()[scaled.image().rgba().len() - 3..],
        &[28, 44, 255]
    );
    session.move_to_pixel(scaled, 5, 3).await.unwrap();
    count += 1;
    let events = eis.wait_event("absolute", count).await;
    let last = events.iter().rfind(|e| e["event"] == "absolute").unwrap();
    assert_eq!(
        (last["x"].as_f64(), last["y"].as_f64()),
        (Some(500.0), Some(500.0))
    );
    let stale = session.observe().unwrap().unwrap().present(11, 9).unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(session
        .move_to_pixel(stale, 0, 0)
        .await
        .unwrap_err()
        .contains("expired"));
    let closing = session.observe().unwrap().unwrap().present(11, 9).unwrap();
    session.stop().await.unwrap();
    assert!(session.move_to_pixel(closing, 0, 0).await.is_err());
    let events = eis.wait_event("disconnect", 1).await;
    assert_eq!(
        events.iter().filter(|e| e["event"] == "absolute").count(),
        17
    );
    assert!(!events
        .iter()
        .any(|e| e["event"] == "key" || e["event"] == "button"));
    native.no_capture_node();
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(0);
    println!("PASS presented pixels: eight inverse orientations + crop + resize + scaled centres + DPI + negative bounds/epoch/expiry/Stop + 17 exact EI motions");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires both owned native PipeWire and libeis fixtures"]
async fn static_source_model_delay_supersession_invalid_buffers_and_loss() {
    let mut native = Native::start().await;
    let eis = super::eis::NativeEis::start("owned-fixture-region").await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    *fixture.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let mut session = PortalSession::on_native_test_bus(
        PortalOptions::new("static-model-lifecycle".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    super::eis::capabilities(&session).await;
    native.link(None).await;
    next_frame(&session, 0).await;
    native.source_command(b's').await;
    // Let already queued native buffers drain. Subsequent checks require no new
    // sequence, no changed pixels, and no retimestamping throughout both waits.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let original = session.frame().unwrap().unwrap();
    let superseded = session.observe().unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(2200)).await;
    native.assert_source_streaming();
    let observation = session.observe().unwrap().unwrap();
    assert_eq!(observation.frame().sequence, original.sequence);
    assert_eq!(observation.frame().captured_at, original.captured_at);
    assert!(observation.issued_at() > original.captured_at);
    assert_eq!(
        observation.expires_at() - observation.issued_at(),
        Duration::from_secs(60)
    );
    assert!(session
        .input_observed(
            superseded,
            InputAction::Key {
                code: 31,
                pressed: true
            }
        )
        .await
        .unwrap_err()
        .contains("superseded"));
    let image = observation.present(1, 1).unwrap();
    tokio::time::sleep(Duration::from_millis(2200)).await;
    native.assert_source_streaming();
    let retained = session.frame().unwrap().unwrap();
    assert_eq!(retained.sequence, original.sequence);
    assert_eq!(retained.captured_at, original.captured_at);
    assert_eq!(retained.rgba, original.rgba);
    session.move_to_pixel(image, 0, 0).await.unwrap();
    eis.wait_event("absolute", 1).await;
    println!("PASS static delivery retained: sequence={} delivery_age_ms={} no new buffers across pre-observation and model-delay waits", original.sequence, original.captured_at.elapsed().as_millis());

    let mut count = 1;
    for mode in *b"gce" {
        let stale = session.observe().unwrap().unwrap();
        let sequence = stale.frame().sequence;
        native.source_command(mode).await;
        let deadline = Instant::now() + Duration::from_secs(3);
        while session.frame().unwrap().is_some() {
            assert!(
                Instant::now() < deadline,
                "invalid native buffer retained old pixels"
            );
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        assert!(session.observe().unwrap().is_none());
        native.assert_source_streaming();
        native.source_command(b's').await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(session.frame().unwrap().is_none());
        native.source_command(b'r').await;
        next_frame(&session, sequence).await;
        assert!(session
            .input_observed(
                stale,
                InputAction::Key {
                    code: 31,
                    pressed: true
                }
            )
            .await
            .unwrap_err()
            .contains("geometry changed"));
        session
            .move_to_pixel(
                session.observe().unwrap().unwrap().present(1, 1).unwrap(),
                0,
                0,
            )
            .await
            .unwrap();
        count += 1;
        eis.wait_event("absolute", count).await;
    }
    native.source_command(b's').await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    session
        .input_observed(
            session.observe().unwrap().unwrap(),
            InputAction::Key {
                code: 30,
                pressed: true,
            },
        )
        .await
        .unwrap();
    eis.wait_event("key", 1).await;
    let revoked = session.observe().unwrap().unwrap();
    // Actual process/socket loss is not simulated with a stale state file.
    native.source.0.kill().unwrap();
    native.source.0.wait().unwrap();
    eis.wait_event("disconnect", 1).await;
    assert!(session.frame().is_err());
    assert!(session
        .input_observed(
            revoked,
            InputAction::Key {
                code: 31,
                pressed: true
            }
        )
        .await
        .is_err());
    session.stop().await.unwrap();
    let events = eis.events();
    let motions: Vec<_> = events.iter().filter(|e| e["event"] == "absolute").collect();
    assert_eq!(motions.len(), 4);
    assert!(motions.iter().all(|e| e["x"] == 500.0 && e["y"] == 500.0));
    let keys: Vec<_> = events.iter().filter(|e| e["event"] == "key").collect();
    assert_eq!(keys.len(), 2);
    assert!(keys.iter().all(|e| e["code"] == 30));
    assert_eq!(keys[0]["pressed"], true);
    assert_eq!(keys[1]["pressed"], false);
    native.no_capture_node();
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(0);
    println!("PASS static lifecycle: supersession + delayed model + GAP/corrupt/empty ABA + 4 exact EI motions + source loss neutralizes held key");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native PipeWire daemon/source acceptance; CI builds the owned C source"]
async fn owned_pipewire_pixels_identity_and_revocation() {
    for ending in ["stop", "portal", "source", "daemon"] {
        let mut native = Native::start().await;
        let fixture = Fixture::new(Mode::Normal).await;
        *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
        let mut session = PortalSession::on_capture_test_bus(
            PortalOptions::new(format!("real-pw-{ending}"), SourceKind::Monitor),
            fixture.bus.address.clone(),
        )
        .unwrap();
        session.ready().await.unwrap();
        assert!(session.frame().unwrap().is_none());
        native.link(None).await;
        let frame = next_frame(&session, 0).await;
        assert_eq!((frame.buffer_width, frame.buffer_height), (32, 24));
        assert_eq!(
            frame.region,
            PixelRegion {
                x: 2,
                y: 2,
                width: 28,
                height: 20
            },
            "native VideoCrop metadata was not applied"
        );
        assert_eq!(frame.pipewire_serial, native.grant.serial);
        assert_eq!(frame.run_id, format!("real-pw-{ending}"));
        assert_eq!(
            frame.rgba.len(),
            (frame.region.width * frame.region.height * 4) as usize
        );
        for y in 0..frame.region.height as usize {
            for x in 0..frame.region.width as usize {
                let pixel = &frame.rgba[(y * frame.region.width as usize + x) * 4..][..4];
                assert_eq!(pixel[1], (y + frame.region.y as usize) as u8);
                assert_eq!(pixel[2], (x + frame.region.x as usize) as u8);
                assert_eq!(pixel[3], 255);
            }
        }
        let second = next_frame(&session, frame.sequence).await;
        assert_ne!(
            second.rgba[0], frame.rgba[0],
            "not an updated native source frame"
        );
        assert_eq!(second.format_generation, frame.format_generation);
        if ending == "stop" {
            // Signal only the exact retained fixture child; never a desktop PID.
            assert_eq!(
                unsafe { libc::kill(native.source.0.id() as i32, libc::SIGUSR1) },
                0
            );
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut after = second.sequence;
            loop {
                let resized = next_frame(&session, after).await;
                after = resized.sequence;
                if resized.buffer_width == 48 {
                    assert_eq!(resized.buffer_height, 32);
                    assert_eq!(
                        resized.region,
                        PixelRegion {
                            x: 2,
                            y: 2,
                            width: 44,
                            height: 28
                        }
                    );
                    assert!(resized.format_generation > frame.format_generation);
                    assert_eq!(resized.rgba.len(), 44 * 28 * 4);
                    eprintln!("PASS native format renegotiation and crop after resize");
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "native source resize did not renegotiate"
                );
            }
        }
        match ending {
            "stop" => session.stop().await.unwrap(),
            "portal" => {
                let path = fixture.shared.sessions.lock().unwrap()[0].clone();
                fixture
                    .server
                    .emit_signal(
                        None::<&str>,
                        path.as_str(),
                        SESSION,
                        "Closed",
                        &(Dict::new(),),
                    )
                    .await
                    .unwrap();
            }
            "source" => {
                native.source.0.kill().unwrap();
                native.source.0.wait().unwrap();
            }
            "daemon" => {
                native.daemon.0.kill().unwrap();
                native.daemon.0.wait().unwrap();
            }
            _ => unreachable!(),
        }
        let reason = terminal(&mut session.subscribe()).await;
        assert!(
            session.frame().is_err(),
            "revoked frame remained retrievable"
        );
        session.stop().await.unwrap();
        fixture.shared.assert_peers_closed(1);
        fixture.assert_sessions_closed();
        if ending != "daemon" {
            native.no_capture_node();
        }
        eprintln!("PASS actual pixels + {ending} + native owner cleanup: {reason:?}");
    }
    let native = Native::start().await;
    let fixture = Fixture::new(Mode::Normal).await;
    let mut wrong = native.grant.clone();
    wrong.serial += 1;
    *fixture.shared.native.lock().unwrap() = Some(wrong);
    let mut session = PortalSession::on_capture_test_bus(
        PortalOptions::new("wrong-serial".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    let reason = terminal(&mut session.subscribe()).await;
    assert!(
        matches!(reason, CloseReason::Failed(ref s) if s.contains("identity mismatch")),
        "{reason:?}"
    );
    assert!(session.frame().is_err());
    session.stop().await.unwrap();
    fixture.shared.assert_peers_closed(1);
    native.no_capture_node();
    eprintln!("PASS actual registry rejects stale serial without creating capture node");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native PipeWire daemon/source acceptance; CI builds the owned C source"]
async fn concurrent_native_capture_runs_do_not_share_authority_or_retirement() {
    let native = Native::start().await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    let mut first = PortalSession::on_capture_test_bus(
        PortalOptions::new("native-one".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    first.ready().await.unwrap();
    let first_node = native.link(None).await;
    let one = next_frame(&first, 0).await;
    let mut second = PortalSession::on_capture_test_bus(
        PortalOptions::new("native-two".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    second.ready().await.unwrap();
    let second_node = native.link(Some(first_node)).await;
    assert_ne!(first_node, second_node);
    let two = next_frame(&second, 0).await;
    assert_eq!(one.run_id, "native-one");
    assert_eq!(two.run_id, "native-two");
    first.stop().await.unwrap();
    assert!(first.frame().is_err());
    let still_live = next_frame(&second, two.sequence).await;
    assert_eq!(still_live.run_id, "native-two");
    assert!(matches!(second.state(), SessionState::Granted(_)));
    // Drop must also retire the real consumer, not just the fake portal fds.
    let mut closing = second.subscribe();
    drop(second);
    assert_eq!(terminal(&mut closing).await, CloseReason::Stopped);
    fixture.shared.assert_peers_closed(2);
    fixture.assert_sessions_closed();
    native.no_capture_node();
    eprintln!(
        "PASS two actual consumers, run-tagged frames, isolated Stop, Drop joins native owner"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native PipeWire daemon/source acceptance; CI builds the owned C source"]
async fn cancelling_stop_wait_does_not_orphan_real_pipewire_owner() {
    let native = Native::start().await;
    let fixture = Fixture::new(Mode::SlowCleanup).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    let mut session = PortalSession::on_capture_test_bus(
        PortalOptions::new("cancel-stop-native".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    native.link(None).await;
    next_frame(&session, 0).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(5), session.stop())
            .await
            .is_err()
    );
    assert!(session.frame().is_err());
    tokio::time::timeout(Duration::from_secs(3), session.stop())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        session.state(),
        SessionState::Closed(CloseReason::Stopped)
    ));
    native.no_capture_node();
    fixture.shared.assert_peers_closed(1);
    eprintln!("PASS cancelled Stop waiter retains actual native owner through cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native PipeWire daemon/source acceptance; CI builds the owned C source"]
async fn stop_waits_for_owned_pipewire_retirement_acknowledgement() {
    let native = Native::start().await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    let mut session = PortalSession::on_capture_test_bus(
        PortalOptions::new("retirement-ack".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    native.link(None).await;
    next_frame(&session, 0).await;
    let paused = pause_daemon(&native.daemon).await;
    let result = tokio::time::timeout(Duration::from_millis(150), session.stop()).await;
    assert!(
        result.is_err(),
        "Stop returned before the owned daemon could process retirement"
    );
    assert!(session.frame().is_err());
    assert!(matches!(session.state(), SessionState::Closing(_)));
    drop(paused);
    tokio::time::timeout(Duration::from_secs(3), session.stop())
        .await
        .unwrap()
        .unwrap();
    native.no_capture_node();
    fixture.shared.assert_peers_closed(1);
    fixture.assert_sessions_closed();
    eprintln!("PASS Stop waits for owned PipeWire retirement acknowledgement, cancelled waiter retains owner");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native PipeWire daemon/source acceptance; CI builds the owned C source"]
async fn retirement_acknowledgement_deadline_revokes_and_joins_without_false_success() {
    let native = Native::start().await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    let mut session = PortalSession::on_capture_test_bus(
        PortalOptions::new("retirement-ack-deadline".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    native.link(None).await;
    next_frame(&session, 0).await;
    let paused = pause_daemon(&native.daemon).await;
    tokio::time::timeout(Duration::from_secs(4), session.stop())
        .await
        .unwrap()
        .unwrap();
    assert!(session.frame().is_err());
    assert!(
        matches!(session.state(), SessionState::Closed(CloseReason::Failed(ref message))
        if message.contains("PipeWire retirement acknowledgement deadline")),
        "{:?}",
        session.state()
    );
    // A failed acknowledgement is not remote-retirement proof. Local Stop
    // still joins the original native owner and closes its granted descriptors.
    fixture.assert_sessions_closed();
    fs::write(
        native.root.join("retirement-audit.json"),
        serde_json::json!({
            "outcome": "acknowledgement-deadline", "nativeOwnerJoined": true,
            "remoteRetirementAcknowledged": false
        })
        .to_string(),
    )
    .unwrap();
    drop(paused);
    eprintln!("PASS stalled daemon deadline is explicit failure, not successful retirement");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native PipeWire daemon/source acceptance; CI builds the owned C source"]
async fn daemon_loss_during_retirement_cannot_be_reported_as_acknowledged_stop() {
    let mut native = Native::start().await;
    let fixture = Fixture::new(Mode::Normal).await;
    *fixture.shared.native.lock().unwrap() = Some(native.grant.clone());
    let mut session = PortalSession::on_capture_test_bus(
        PortalOptions::new("retirement-daemon-loss".into(), SourceKind::Monitor),
        fixture.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    native.link(None).await;
    next_frame(&session, 0).await;
    let paused = pause_daemon(&native.daemon).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(150), session.stop())
            .await
            .is_err()
    );
    // SIGKILL the exact retained child while stopped, so no acknowledgement can
    // race ahead of the test. Never kill an unrelated daemon by name.
    assert_eq!(
        unsafe { libc::kill(paused.0 .0.id() as libc::pid_t, libc::SIGKILL) },
        0
    );
    drop(paused);
    native.daemon.0.wait().unwrap();
    tokio::time::timeout(Duration::from_secs(3), session.stop())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(session.state(), SessionState::Closed(CloseReason::Failed(ref message))
        if message.contains("PipeWire retirement")),
        "{:?}",
        session.state()
    );
    assert!(session.frame().is_err());
    fixture.assert_sessions_closed();
    eprintln!("PASS daemon loss during retirement joins owner and reports missing acknowledgement");
}
