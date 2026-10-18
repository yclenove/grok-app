use super::*;

fn query(request: DriverKind, scene: Option<&str>, entry: Entry, os: OsKind) -> Query<'_> {
    Query {
        request,
        scene,
        entry,
        os,
        features: Features {
            native_windows: false,
            native_x11: false,
            native_wayland: false,
        },
        debug: true,
        allow_fake: false,
    }
}

fn linux_x11() -> OsKind {
    OsKind::Linux {
        session: SessionKind::X11,
    }
}

#[test]
fn omitted_auto_and_native_fail_closed_for_every_feature_switch() {
    for compiled in [false, true] {
        for request in [DriverKind::Auto, DriverKind::Native] {
            for (os, feature) in [
                (OsKind::Windows, "native-windows"),
                (linux_x11(), "native-x11"),
                (
                    OsKind::Linux {
                        session: SessionKind::Wayland { display: false },
                    },
                    "native-wayland",
                ),
            ] {
                let mut q = query(request, None, Entry::InProcess, os);
                q.features = Features {
                    native_windows: compiled && feature == "native-windows",
                    native_x11: compiled && feature == "native-x11",
                    native_wayland: compiled && feature == "native-wayland",
                };
                let plan = decide(q);
                if compiled && feature == "native-x11" {
                    let Plan::NativeX11 { disclosure } = plan else {
                        panic!("expected native x11, got {plan:?}");
                    };
                    assert_eq!(disclosure.selected, "native");
                    assert!(
                        disclosure.native_disabled.contains("selected native-x11"),
                        "{}",
                        disclosure.native_disabled
                    );
                    assert!(disclosure.native_disabled.contains("native-wayland"));
                    assert!(!disclosure.native_disabled.contains("not assembled"));
                    continue;
                }
                if compiled && feature == "native-windows" {
                    let Plan::NativeWindows { disclosure } = plan else {
                        panic!("expected native windows, got {plan:?}");
                    };
                    assert_eq!(disclosure.selected, "native");
                    assert!(
                        disclosure
                            .native_disabled
                            .contains("selected native-windows"),
                        "{}",
                        disclosure.native_disabled
                    );
                    assert!(!disclosure.native_disabled.contains("not assembled"));
                    assert!(!disclosure.note().contains("fake"));
                    continue;
                }
                let Plan::Refuse { message } = plan else {
                    panic!("driver was selected: {plan:?}");
                };
                assert!(message.contains(feature), "{message}");
                if compiled {
                    assert!(message.contains("not assembled"), "{message}");
                } else {
                    assert!(message.contains("not compiled"), "{message}");
                }
            }
        }
    }
}

#[test]
fn windows_agent_does_not_assemble_and_other_entries_do() {
    let mut agent = query(DriverKind::Auto, None, Entry::Agent, OsKind::Windows);
    agent.features.native_windows = true;
    match decide(agent) {
        Plan::Refuse { message } => assert!(message.contains("helper agent"), "{message}"),
        other => panic!("selected {other:?}"),
    }
    for entry in [
        Entry::Bare,
        Entry::McpStdio,
        Entry::Engine,
        Entry::InProcess,
    ] {
        for request in [DriverKind::Auto, DriverKind::Native] {
            let mut selected = query(request, None, entry, OsKind::Windows);
            selected.features.native_windows = true;
            assert!(
                matches!(decide(selected), Plan::NativeWindows { .. }),
                "{request:?} {entry:?}"
            );
        }
    }
}

#[test]
fn fake_on_compiled_windows_does_not_claim_the_route_is_unassembled() {
    let mut q = query(DriverKind::Fake, None, Entry::InProcess, OsKind::Windows);
    q.features.native_windows = true;
    match decide(q) {
        Plan::Fake { disclosure, .. } => {
            assert_eq!(disclosure.selected, "fake");
            assert!(disclosure.native_disabled.contains("did not select"));
            assert!(disclosure.native_disabled.contains("native-windows"));
            assert!(!disclosure.native_disabled.contains("not assembled"));
        }
        other => panic!("{other:?}"),
    }
    q.features.native_windows = false;
    q.request = DriverKind::Native;
    match decide(q) {
        Plan::Refuse { message } => {
            assert!(message.contains("not compiled"), "{message}");
            assert!(message.contains("native-windows"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn linux_x11_agent_does_not_assemble_in_process() {
    let mut q = query(DriverKind::Auto, None, Entry::Agent, linux_x11());
    q.features.native_x11 = true;
    match decide(q) {
        Plan::Refuse { message } => {
            assert!(message.contains("helper agent"), "{message}");
        }
        other => panic!("selected {other:?}"),
    }
    for entry in [Entry::Bare, Entry::McpStdio] {
        let mut selected = query(DriverKind::Native, None, entry, linux_x11());
        selected.features.native_x11 = true;
        assert!(
            matches!(decide(selected), Plan::NativeX11 { .. }),
            "{entry:?}"
        );
    }
}

#[test]
fn fake_on_compiled_x11_does_not_claim_the_route_is_unassembled() {
    let mut q = query(DriverKind::Fake, None, Entry::InProcess, linux_x11());
    q.features.native_x11 = true;
    match decide(q) {
        Plan::Fake { disclosure, .. } => {
            assert_eq!(disclosure.selected, "fake");
            assert!(disclosure.native_disabled.contains("did not select"));
            assert!(disclosure.native_disabled.contains("native-x11"));
            assert!(!disclosure.native_disabled.contains("not assembled"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn wayland_with_display_does_not_select_x11_even_when_x11_is_compiled() {
    let mut q = query(
        DriverKind::Auto,
        None,
        Entry::InProcess,
        OsKind::Linux {
            session: SessionKind::Wayland { display: true },
        },
    );
    q.features.native_x11 = true;
    let Plan::Refuse { message } = decide(q) else {
        panic!("x11 route was selected");
    };
    assert!(message.contains("X11 route is not selected"), "{message}");
    assert!(message.contains("native-wayland"), "{message}");
    assert!(!message.contains("native-x11 is compiled but"), "{message}");
}

#[test]
fn headless_and_scene_mismatch_refuse_before_any_driver() {
    let plan = decide(query(
        DriverKind::Auto,
        None,
        Entry::InProcess,
        OsKind::Linux {
            session: SessionKind::Headless,
        },
    ));
    match plan {
        Plan::Refuse { message } => {
            assert!(message.contains("No safe native route"), "{message}")
        }
        other => panic!("selected {other:?}"),
    }
    for request in [DriverKind::Auto, DriverKind::Native] {
        match decide(query(request, Some("form"), Entry::InProcess, linux_x11())) {
            Plan::Refuse { message } => {
                assert_eq!(message, "--scene is valid only with --driver fake")
            }
            other => panic!("selected {other:?}"),
        }
    }
}

#[test]
fn fake_keeps_the_debug_or_allow_gate_and_defaults_scene() {
    let mut q = query(DriverKind::Fake, None, Entry::InProcess, linux_x11());
    match decide(q) {
        Plan::Fake { scene, disclosure } => {
            assert_eq!(scene, "form");
            assert_eq!(disclosure.selected, "fake");
            assert!(disclosure.native_disabled.contains("native-x11"));
            assert!(disclosure.note().contains("fake"));
        }
        other => panic!("{other:?}"),
    }
    q.scene = Some("chat");
    match decide(q) {
        Plan::Fake { scene, .. } => assert_eq!(scene, "chat"),
        other => panic!("{other:?}"),
    }
    q.debug = false;
    q.allow_fake = false;
    assert!(matches!(decide(q), Plan::Refuse { .. }));
    q.allow_fake = true;
    assert!(matches!(decide(q), Plan::Fake { .. }));
    q.request = DriverKind::Auto;
    q.scene = None;
    q.allow_fake = true;
    assert!(
        matches!(decide(q), Plan::Refuse { .. }),
        "GCU_ALLOW_FAKE must not select fake for auto"
    );
}

#[test]
fn macos_auto_routes_follow_the_entry_and_fake_stays_explicit() {
    let os = OsKind::Macos;
    assert!(matches!(
        decide(query(DriverKind::Auto, None, Entry::McpStdio, os)),
        Plan::HelperProxy { .. }
    ));
    assert!(matches!(
        decide(query(DriverKind::Native, None, Entry::Agent, os)),
        Plan::HelperAgent { .. }
    ));
    assert!(matches!(
        decide(query(DriverKind::Auto, None, Entry::Bare, os)),
        Plan::Onboarding { .. }
    ));
    for request in [DriverKind::Auto, DriverKind::Native] {
        match decide(query(request, None, Entry::InProcess, os)) {
            Plan::Refuse { message } => {
                assert_eq!(message, crate::host::macos_wire::TERMINAL_NATIVE_REFUSAL);
                assert_eq!(crate::exit::Failure::arguments(message).code, 2);
            }
            other => panic!("{other:?}"),
        }
    }
    for request in [DriverKind::Auto, DriverKind::Native] {
        match decide(query(request, None, Entry::Engine, os)) {
            Plan::MacosEngine { disclosure } => {
                assert!(
                    disclosure.native_disabled.contains("macOS"),
                    "{disclosure:?}"
                )
            }
            other => panic!("{other:?}"),
        }
    }
    let mut enabled = query(DriverKind::Auto, None, Entry::Engine, os);
    enabled.features = Features {
        native_windows: true,
        native_x11: true,
        native_wayland: true,
    };
    assert!(matches!(decide(enabled), Plan::MacosEngine { .. }));
    let mut terminal = query(DriverKind::Native, None, Entry::InProcess, os);
    terminal.features = enabled.features;
    match decide(terminal) {
        Plan::Refuse { message } => {
            assert_eq!(message, crate::host::macos_wire::TERMINAL_NATIVE_REFUSAL)
        }
        other => panic!("{other:?}"),
    }
    match decide(query(DriverKind::Fake, None, Entry::Engine, os)) {
        Plan::Fake { disclosure, .. } => assert_eq!(disclosure.selected, "fake"),
        other => panic!("{other:?}"),
    }
    match decide(query(
        DriverKind::Auto,
        None,
        Entry::Engine,
        OsKind::Windows,
    )) {
        Plan::Refuse { message } => assert!(message.contains("native-windows"), "{message}"),
        other => panic!("{other:?}"),
    }
    match decide(query(DriverKind::Fake, None, Entry::McpStdio, os)) {
        Plan::Fake { disclosure, .. } => assert_eq!(disclosure.selected, "fake"),
        other => panic!("{other:?}"),
    }
    match decide(query(DriverKind::Auto, Some("form"), Entry::Agent, os)) {
        Plan::Refuse { message } => assert!(message.contains("--scene")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn release_fake_without_allow_is_distinct_from_native_failure() {
    let mut q = query(DriverKind::Fake, None, Entry::Bare, OsKind::Windows);
    q.debug = false;
    match decide(q) {
        Plan::Refuse { message } => assert!(message.contains("GCU_ALLOW_FAKE")),
        other => panic!("{other:?}"),
    }
}
