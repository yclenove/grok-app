//! Real libei -> owned libeis, with the fd granted through private D-Bus.
//! No user's compositor, input device, XWayland or Notify* injection involved.
use super::*;
use serde_json::Value as Json;
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::PathBuf, time::Instant};

pub(super) struct NativeEis {
    root: PathBuf,
    pub(super) socket: PathBuf,
    child: Child,
}
impl NativeEis {
    pub(super) async fn start(mapping: &str) -> Self {
        let exe = std::env::var_os("GROK_CU_EIS_SERVER")
            .expect("compile tests/eis-server.c; native EI tests are not skippable");
        let root =
            std::env::temp_dir().join(format!("grok-cu-eis-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.join("owned-eis.sock");
        let mut child = owned_child(
            Command::new(exe)
                .arg(&socket)
                .arg(mapping)
                .env_remove("LIBEI_SOCKET")
                .env_remove("WAYLAND_DISPLAY")
                .env_remove("DISPLAY")
                .stdin(Stdio::piped())
                .stdout(fs::File::create(root.join("events.jsonl")).unwrap())
                .stderr(fs::File::create(root.join("stderr.log")).unwrap()),
        )
        .spawn()
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !socket.exists() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "libeis fixture failed: {}",
                fs::read_to_string(root.join("stderr.log")).unwrap()
            );
            assert!(Instant::now() < deadline, "libeis startup timeout");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Self {
            root,
            socket,
            child,
        }
    }
    pub(super) fn events(&self) -> Vec<Json> {
        fs::read_to_string(self.root.join("events.jsonl"))
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
    pub(super) fn command(&mut self, c: u8) {
        self.child.stdin.as_mut().unwrap().write_all(&[c]).unwrap();
    }
    pub(super) async fn wait_event(&self, name: &str, count: usize) -> Vec<Json> {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let events = self.events();
            if events.iter().filter(|e| e["event"] == name).count() >= count {
                return events;
            }
            assert!(
                Instant::now() < deadline,
                "missing {name}: {events:?}; stderr={}",
                fs::read_to_string(self.root.join("stderr.log")).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl Drop for NativeEis {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        eprintln!("owned libeis logs: {}", self.root.display());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native libeis server"]
async fn real_ei_pause_resume_requires_new_generation_and_runs_are_isolated() {
    let f1 = Fixture::new(Mode::Normal).await;
    let f2 = Fixture::new(Mode::Normal).await;
    let mut e1 = NativeEis::start("owned-fixture-region").await;
    let e2 = NativeEis::start("owned-fixture-region").await;
    let mut s1 = start(&f1, &e1, "ei-one").await;
    let mut s2 = start(&f2, &e2, "ei-two").await;
    let c1 = capabilities(&s1).await;
    let c2 = capabilities(&s2).await;
    s2.input(
        c2.generation,
        InputAction::Key {
            code: 31,
            pressed: true,
        },
    )
    .await
    .unwrap();
    // libeis 1.3.901's pause API only acts in RESUMED, not EMULATING.
    // Exercise an actual pause, not an inert call while a key is held.
    e1.command(b'p');
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if matches!(s1.input_state(), InputState::Ready(ref c) if !c.keyboard && c.generation > c1.generation)
        {
            break;
        }
        assert!(Instant::now() < deadline, "pause was not received");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(s1
        .input(
            c1.generation,
            InputAction::Key {
                code: 30,
                pressed: true
            }
        )
        .await
        .is_err());
    if let InputState::Ready(c) = s1.input_state() {
        assert!(s1
            .input(
                c.generation,
                InputAction::Key {
                    code: 30,
                    pressed: true
                }
            )
            .await
            .is_err());
    }
    e1.command(b'r');
    let resumed = capabilities(&s1).await;
    assert!(resumed.generation > c1.generation);
    s1.input(
        resumed.generation,
        InputAction::Key {
            code: 30,
            pressed: true,
        },
    )
    .await
    .unwrap();
    s1.input(resumed.generation, InputAction::ReleaseAll)
        .await
        .unwrap();
    s1.input(
        resumed.generation,
        InputAction::Key {
            code: 30,
            pressed: true,
        },
    )
    .await
    .unwrap();
    s1.stop().await.unwrap();
    let first = e1.wait_event("disconnect", 1).await;
    assert_eq!(
        first
            .iter()
            .filter(|e| e["event"] == "key" && e["code"] == 30)
            .map(|e| e["pressed"].as_bool().unwrap())
            .collect::<Vec<_>>(),
        [true, false, true, false]
    );
    assert!(matches!(s2.input_state(), InputState::Ready(_)));
    assert!(!e2
        .events()
        .iter()
        .any(|e| e["event"] == "disconnect" || (e["event"] == "key" && e["pressed"] == false)));
    let receipt = s2
        .input(c2.generation, InputAction::Relative { dx: 2.0, dy: 1.0 })
        .await
        .unwrap();
    assert_eq!(receipt.run_id, "ei-two");
    s2.stop().await.unwrap();
    assert!(e2
        .wait_event("disconnect", 1)
        .await
        .iter()
        .any(|e| e["event"] == "key" && e["code"] == 31 && e["pressed"] == false));
    println!(
        "PASS real EI pause/resume generation, repeated emulation and isolated run retirement"
    );
}
pub(super) async fn capabilities(session: &PortalSession) -> InputCapabilities {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let InputState::Ready(caps) = session.input_state() {
            if caps.keyboard && caps.relative_pointer && caps.buttons && caps.scroll {
                return caps;
            }
        }
        assert!(
            !matches!(
                session.state(),
                SessionState::Closing(_) | SessionState::Closed(_)
            ),
            "input owner failed: {:?}",
            session.state()
        );
        assert!(
            Instant::now() < deadline,
            "EI capabilities unavailable: {:?}",
            session.input_state()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
async fn start(f: &Fixture, eis: &NativeEis, run: &str) -> PortalSession {
    *f.shared.eis_socket.lock().unwrap() = Some(eis.socket.clone());
    let mut session = PortalSession::on_input_test_bus(
        PortalOptions::new(run.into(), SourceKind::Monitor),
        f.bus.address.clone(),
    )
    .unwrap();
    session.ready().await.unwrap();
    session
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native libeis server"]
async fn real_ei_physical_pointer_is_not_a_logical_pixel_route() {
    let f = Fixture::new(Mode::NoMapping).await;
    let eis = NativeEis::start("physical-relative").await;
    let mut s = start(&f, &eis, "physical-unit-guard").await;
    let deadline = Instant::now() + Duration::from_secs(4);
    let caps = loop {
        // Connect + seat + three added/resumed devices, including the physical
        // relative device. Waiting for the complete generation prevents a
        // passing assertion caused only by incomplete startup.
        if let InputState::Ready(c) = s.input_state() {
            if c.generation >= 8 {
                break c;
            }
        }
        assert!(
            Instant::now() < deadline,
            "physical EI device not negotiated"
        );
        assert!(
            matches!(s.state(), SessionState::Granted(_)),
            "{:?}",
            s.state()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(caps.keyboard && caps.buttons && caps.scroll);
    assert!(!caps.relative_pointer);
    assert!(s
        .input(
            caps.generation,
            InputAction::Relative { dx: 20.0, dy: 10.0 }
        )
        .await
        .is_err());
    s.stop().await.unwrap();
    let events = eis.wait_event("disconnect", 1).await;
    assert!(!events
        .iter()
        .any(|e| e["event"] == "relative" || e["event"] == "start"));
    println!("PASS physical EI millimeters cannot receive logical-pixel motion");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned tests/eis-server.c compiled against actual libeis"]
async fn real_ei_input_coordinates_scancodes_scroll_and_neutral_stop() {
    let f = Fixture::new(Mode::Normal).await;
    let eis = NativeEis::start("owned-fixture-region").await;
    let mut s = start(&f, &eis, "actual-ei-input").await;
    let caps = capabilities(&s).await;
    let region = caps.absolute_region.unwrap();
    assert_eq!(
        (region.x, region.y, region.width, region.height),
        (100, 200, 800, 600)
    );
    assert_eq!(region.physical_scale, 2.0);
    let actions = [
        InputAction::Absolute { x: 10.5, y: 20.25 },
        InputAction::Relative { dx: -4.0, dy: 3.0 },
        InputAction::Key {
            code: 30,
            pressed: true,
        },
        InputAction::Button {
            code: 0x110,
            pressed: true,
        },
        InputAction::Scroll { dx: 0.0, dy: 12.5 },
        InputAction::ScrollDiscrete { dx: -120, dy: 240 },
    ];
    for (index, action) in actions.into_iter().enumerate() {
        let receipt = s.input(caps.generation, action).await.unwrap();
        assert_eq!(receipt.run_id, "actual-ei-input");
        assert_eq!(receipt.sequence, index as u64 + 1);
    }
    assert!(s
        .input(
            caps.generation,
            InputAction::Key {
                code: 30,
                pressed: true
            }
        )
        .await
        .is_err());
    assert!(s
        .input(
            caps.generation,
            InputAction::Key {
                code: 31,
                pressed: false
            }
        )
        .await
        .is_err());
    assert!(s
        .input(caps.generation, InputAction::Absolute { x: 800.0, y: 0.0 })
        .await
        .is_err());
    eis.wait_event("discrete", 1).await;
    s.stop().await.unwrap();
    assert_eq!(s.input_state(), InputState::Revoked);
    let events = eis.wait_event("disconnect", 1).await;
    assert!(events
        .iter()
        .any(|e| e["event"] == "absolute" && e["x"] == 110.5 && e["y"] == 220.25));
    assert!(events
        .iter()
        .any(|e| e["event"] == "relative" && e["x"] == -4.0 && e["y"] == 3.0));
    assert!(events
        .iter()
        .any(|e| e["event"] == "scroll" && e["y"] == 12.5));
    assert!(events
        .iter()
        .any(|e| e["event"] == "discrete" && e["x"] == -120 && e["y"] == 240));
    for (kind, code) in [("key", 30), ("button", 0x110)] {
        let transitions: Vec<_> = events
            .iter()
            .filter(|e| e["event"] == kind && e["code"] == code)
            .map(|e| e["pressed"].as_bool().unwrap())
            .collect();
        assert_eq!(transitions, [true, false]);
    }
    assert_eq!(
        events.iter().filter(|e| e["event"] == "absolute").count(),
        1
    );
    assert!(events.iter().any(|e| e["event"] == "scroll-cancel"));
    assert_eq!(events.iter().filter(|e| e["event"] == "start").count(), 3);
    assert_eq!(events.iter().filter(|e| e["event"] == "stop").count(), 3);
    f.shared.assert_peers_closed(1);
    println!("PASS actual libei scancodes, matched-region coordinates, two scroll types and neutral Stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native libeis server"]
async fn real_ei_mapping_ambiguity_topology_and_missing_mapping_fail_closed() {
    for mode in [Mode::Normal, Mode::NoMapping] {
        let f = Fixture::new(mode).await;
        let mut eis = NativeEis::start("owned-fixture-region").await;
        let mut s = start(&f, &eis, "mapping-ei").await;
        let caps = capabilities(&s).await;
        if mode == Mode::NoMapping {
            assert!(caps.absolute_region.is_none());
            assert!(s
                .input(caps.generation, InputAction::Absolute { x: 1.0, y: 1.0 })
                .await
                .is_err());
        } else {
            eis.command(b'd');
            eis.wait_event("duplicated", 1).await;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if matches!(s.input_state(), InputState::Ready(ref c) if c.generation > caps.generation && c.absolute_region.is_none())
                {
                    break;
                }
                assert!(Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(s
                .input(
                    caps.generation,
                    InputAction::Key {
                        code: 30,
                        pressed: true
                    }
                )
                .await
                .is_err());
            if let InputState::Ready(c) = s.input_state() {
                assert!(s
                    .input(c.generation, InputAction::Absolute { x: 1.0, y: 1.0 })
                    .await
                    .is_err());
            }
        }
        s.stop().await.unwrap();
        assert!(!eis
            .wait_event("disconnect", 1)
            .await
            .iter()
            .any(|e| e["event"] == "absolute" || e["event"] == "key"));
    }
    println!(
        "PASS missing/duplicate EI mapping and stale generation never emit absolute or stale input"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned native libeis server"]
async fn real_ei_revocation_drop_and_cancelled_submission_retire_held_keys() {
    for reason in [
        "drop",
        "portal",
        "cancel",
        "remove",
        "topology",
        "disconnect",
        "cancel-stop",
    ] {
        let f = Fixture::new(if reason == "cancel-stop" {
            Mode::SlowCleanup
        } else {
            Mode::Normal
        })
        .await;
        let mut eis = NativeEis::start("owned-fixture-region").await;
        let mut s = start(&f, &eis, reason).await;
        let caps = capabilities(&s).await;
        s.input(
            caps.generation,
            InputAction::Key {
                code: 42,
                pressed: true,
            },
        )
        .await
        .unwrap();
        eis.wait_event("key", 1).await;
        let mut state = s.subscribe();
        match reason {
            "drop" => drop(s),
            "portal" => {
                let path = f.shared.sessions.lock().unwrap()[0].clone();
                f.server
                    .emit_signal(
                        None::<&str>,
                        path.as_str(),
                        SESSION,
                        "Closed",
                        &(Dict::new(),),
                    )
                    .await
                    .unwrap();
                terminal(&mut state).await;
                s.stop().await.unwrap();
            }
            "cancel" => {
                let mut pending =
                    Box::pin(s.input(caps.generation, InputAction::Relative { dx: 1.0, dy: 1.0 }));
                assert!(futures_util::poll!(pending.as_mut()).is_pending());
                drop(pending);
                assert_eq!(s.input_state(), InputState::Revoked);
                s.stop().await.unwrap();
            }
            "remove" | "disconnect" | "topology" => {
                eis.command(match reason {
                    "remove" => b'k',
                    "topology" => b'd',
                    _ => b'x',
                });
                tokio::time::timeout(Duration::from_secs(4), async {
                    loop {
                        if matches!(*state.borrow(), SessionState::Closed(_)) {
                            break;
                        }
                        state.changed().await.unwrap();
                    }
                })
                .await
                .unwrap();
                assert!(s
                    .input(
                        caps.generation,
                        InputAction::Key {
                            code: 31,
                            pressed: true
                        }
                    )
                    .await
                    .is_err());
                s.stop().await.unwrap();
            }
            "cancel-stop" => {
                assert!(tokio::time::timeout(Duration::from_millis(5), s.stop())
                    .await
                    .is_err());
                s.stop().await.unwrap();
            }
            _ => unreachable!(),
        }
        let events = eis.wait_event("disconnect", 1).await;
        if !matches!(reason, "remove" | "disconnect") {
            assert!(
                events
                    .iter()
                    .any(|e| e["event"] == "key" && e["code"] == 42 && e["pressed"] == false),
                "held key not released before {reason}: {events:?}"
            );
        }
        f.shared.assert_peers_closed(1);
        println!("PASS real EI retirement: {reason}");
    }
}
