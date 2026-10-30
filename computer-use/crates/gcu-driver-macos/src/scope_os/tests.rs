//! Fail-closed identity. No window server, accessibility, or screenshot.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gcu_core::driver::{
    AppQuery, AppRef, Button, Cancel, Driver, DriverAction, DriverError, PointerRoute, WindowRef,
};
use gcu_core::model::{NativeHandle, WindowPoint};
use gcu_core::scope::InstanceEvidence;
use gcu_protocol::capabilities::PrivateApiStatus;
use gcu_protocol::AppInfo;

use super::deliver::{DeliveryFault, PostView};
use super::evidence::{not_proved, parse_cg, Evidence};
use super::facts::{ElementFact, FakeOsFacts, OsFacts, OsWindow};
use super::spi::ax_window_capability;
use crate::api::MacApi;
use crate::driver::MacosDriver;
use crate::input::{
    deliver, Call, EventApi, EventKind, FakeEvents, InputLedger, InputTarget, PostFault, PostGate,
    SyntheticEvent,
};
use crate::testing::{cg_window, rect, signed_facts, FakeMacApi};

fn facts_for(pid: u32, start: u64) -> (Arc<FakeOsFacts>, Arc<Evidence>) {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot");
    facts.set_process(pid, start, Some("/bin/app"));
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    (facts, evidence)
}

fn row(cg: u32, pid: u32, token: Option<u64>, frame: (f64, f64, f64, f64)) -> OsWindow {
    OsWindow {
        cg_id: cg,
        owner_pid: pid,
        layer: 0,
        alpha: 1.0,
        frame: rect(frame.0, frame.1, frame.2, frame.3),
        on_screen: true,
        token,
    }
}

fn view(
    foreground: bool,
    coordinate: bool,
    pid: i32,
    window_id: u32,
    screen: Option<(f64, f64)>,
    window_local: Option<(f64, f64)>,
) -> PostView {
    PostView {
        foreground,
        coordinate,
        pid,
        window_id,
        screen,
        window_local,
    }
}

#[test]
fn app_instance_records_start_and_boot() {
    let (_facts, evidence) = facts_for(4, 1_700_000_000_000_000);
    let instance = evidence.app_pid(4).expect("proved");
    assert_eq!(instance.pid(), 4);
    assert_eq!(instance.start().ticks(), 1_700_000_000_000_000);
    assert_eq!(instance.boot().as_str(), "boot");
    let again = evidence.app_pid(4).expect("same instance");
    assert_eq!(again, instance);
}

#[test]
fn app_instance_exited_pid_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    assert!(evidence.app_pid(4).is_some());
    facts.drop_process(4);
    assert!(evidence.app_pid(4).is_none());
}

#[test]
fn app_instance_reused_pid_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    assert!(evidence.app_pid(4).is_some());
    facts.set_process(4, 200, Some("/bin/app"));
    assert!(evidence.app_pid(4).is_none());
    facts.set_process(4, 100, Some("/bin/app"));
    assert!(evidence.app_pid(4).is_some());
}

#[test]
fn app_instance_boot_change_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    assert!(evidence.app_pid(4).is_some());
    facts.set_boot("other-boot");
    assert!(evidence.app_pid(4).is_none());
}

#[test]
fn app_instance_missing_boot_or_zero_start_or_missing_pid_returns_none() {
    let facts = Arc::new(FakeOsFacts::new());
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    facts.set_process(4, 100, None);
    assert!(evidence.app_pid(4).is_none());
    facts.set_boot("");
    assert!(evidence.app_pid(4).is_none());
    facts.set_boot("boot");
    facts.set_process(4, 0, None);
    assert!(evidence.app_pid(4).is_none());
    assert!(evidence.app_pid(0).is_none());
}

#[test]
fn executable_proof_returns_path_while_reused_pid_stays_none() {
    let (facts, evidence) = facts_for(4, 100);
    assert_eq!(
        evidence.executable_proof(4).expect("path").path(),
        "/bin/app"
    );
    assert!(evidence.app_pid(4).is_some());
    facts.set_process(4, 200, Some("/bin/other"));
    assert!(evidence.app_pid(4).is_none());
    assert_eq!(
        evidence
            .executable_proof(4)
            .expect("path still evidence")
            .path(),
        "/bin/other"
    );
}

#[test]
fn window_binding_same_token_and_owner_keeps_incarnation() {
    let (facts, evidence) = facts_for(4, 100);
    let frame = (0.0, 0.0, 80.0, 40.0);
    facts.set_windows(Some(vec![row(7, 4, Some(1), frame)]));
    let first = evidence.window_binding_cg(7).expect("window");
    facts.set_windows(Some(vec![row(7, 4, Some(1), (3.0, 4.0, 10.0, 10.0))]));
    let second = evidence.window_binding_cg(7).expect("same object");
    assert_eq!(first.incarnation(), 1);
    assert_eq!(second, first);
    assert_eq!(second.native_id(), "7");
    assert_eq!(parse_cg("cg:7:999"), Some(7));
    assert_eq!(evidence.window_binding_cg(7).expect("id").owner().pid(), 4);
    assert!(parse_cg("cg:0:4").is_none());
    assert!(parse_cg("nope").is_none());
}

#[test]
fn window_binding_different_owner_instance_bumps_incarnation() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_process(5, 200, Some("/bin/other"));
    let frame = (0.0, 0.0, 40.0, 40.0);
    facts.set_windows(Some(vec![row(7, 4, Some(1), frame)]));
    let first = evidence.window_binding_cg(7).expect("first owner");
    facts.set_windows(Some(vec![row(7, 5, Some(1), frame)]));
    let second = evidence.window_binding_cg(7).expect("new owner");
    assert_eq!(first.incarnation(), 1);
    assert_eq!(second.incarnation(), 2);
    assert_eq!(second.owner().pid(), 5);
}

#[test]
fn window_binding_disappear_then_reappear_bumps_incarnation() {
    let (facts, evidence) = facts_for(4, 100);
    let frame = (0.0, 0.0, 40.0, 40.0);
    facts.set_windows(Some(vec![row(7, 4, Some(1), frame)]));
    assert_eq!(
        evidence.window_binding_cg(7).expect("seen").incarnation(),
        1
    );
    facts.set_windows(Some(vec![]));
    assert!(evidence.window_binding_cg(7).is_none());
    facts.set_windows(Some(vec![row(7, 4, Some(1), frame)]));
    assert_eq!(
        evidence
            .window_binding_cg(7)
            .expect("reused id")
            .incarnation(),
        2
    );
}

#[test]
fn window_binding_lost_observation_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    let frame = (0.0, 0.0, 40.0, 40.0);
    facts.set_windows(Some(vec![row(7, 4, Some(1), frame)]));
    assert!(evidence.window_binding_cg(7).is_some());
    facts.set_lost(true);
    assert!(evidence.window_binding_cg(7).is_none());
    facts.set_lost(false);
    facts.set_windows(Some(vec![
        row(7, 4, Some(2), frame),
        row(8, 4, Some(3), frame),
    ]));
    assert!(evidence.window_binding_cg(7).is_none());
    assert!(evidence.window_binding_cg(8).is_none());
    facts.set_windows(Some(vec![row(9, 4, Some(4), frame)]));
    assert_eq!(
        evidence
            .window_binding_cg(9)
            .expect("later id")
            .incarnation(),
        1
    );
    assert!(evidence.window_binding_cg(7).is_none());
}

#[test]
fn window_binding_missing_token_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    let frame = (1.0, 2.0, 30.0, 30.0);
    facts.set_windows(Some(vec![row(7, 4, None, frame)]));
    assert!(evidence.window_binding_cg(7).is_none());
    facts.set_windows(Some(vec![row(7, 4, Some(1), frame)]));
    assert!(evidence.window_binding_cg(7).is_none());
}

#[test]
fn window_binding_absent_from_list_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_windows(Some(vec![row(8, 4, Some(1), (0.0, 0.0, 10.0, 10.0))]));
    assert!(evidence.window_binding_cg(7).is_none());
    facts.set_windows(None);
    assert!(evidence.window_binding_cg(8).is_none());
}

#[test]
fn element_binding_forged_handle_returns_none() {
    let (_facts, evidence) = facts_for(4, 100);
    assert!(evidence.element_handle(&NativeHandle::new(7u32)).is_none());
}

#[test]
fn element_binding_unknown_id_returns_none() {
    let (_facts, evidence) = facts_for(4, 100);
    assert!(evidence.element_ids(4, 99).is_none());
}

#[test]
fn element_binding_other_process_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_process(9, 50, None);
    facts.set_windows(Some(vec![row(7, 9, Some(1), (0.0, 0.0, 20.0, 20.0))]));
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 9,
            cg_id: Some(7),
        },
    );
    assert!(evidence.element_ids(4, 3).is_none());
}

#[test]
fn element_binding_unproved_window_returns_none() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 4,
            cg_id: None,
        },
    );
    assert!(evidence.element_ids(4, 3).is_none());
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 4,
            cg_id: Some(7),
        },
    );
    assert!(evidence.element_ids(4, 3).is_none());
}

#[test]
fn element_binding_proved_window_matches_owner() {
    let (facts, evidence) = facts_for(4, 100);
    let frame = (0.0, 0.0, 40.0, 40.0);
    facts.set_windows(Some(vec![
        row(7, 4, Some(1), frame),
        row(8, 4, Some(2), frame),
    ]));
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 4,
            cg_id: Some(7),
        },
    );
    let binding = evidence.element_ids(4, 3).expect("element");
    assert_eq!(binding.window().native_id(), "7");
    assert_eq!(binding.window().owner().pid(), 4);
    let expected = binding.window().clone();
    assert!(evidence.accept_gathered(
        &ElementFact {
            pid: 4,
            cg_id: Some(7)
        },
        &expected
    ));
    assert!(!evidence.accept_gathered(
        &ElementFact {
            pid: 4,
            cg_id: Some(8)
        },
        &expected
    ));
    facts.set_windows(Some(vec![row(7, 9, Some(1), frame)]));
    facts.set_process(9, 50, None);
    assert!(!evidence.accept_gathered(
        &ElementFact {
            pid: 4,
            cg_id: Some(7)
        },
        &expected
    ));
}

#[test]
fn prove_post_point_on_topmost_admitted_window_allows() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_windows(Some(vec![row(7, 4, Some(1), (100.0, 100.0, 40.0, 40.0))]));
    let admitted = evidence.window_binding_cg(7).expect("window");
    let post = view(false, true, 4, 7, None, Some((10.0, 10.0)));
    assert!(evidence.prove_delivery(&admitted, &post).is_ok());
}

#[test]
fn prove_post_occluded_or_other_window_refuses_and_does_not_post() {
    let (facts, evidence) = facts_for(4, 100);
    let frame = (100.0, 100.0, 40.0, 40.0);
    facts.set_windows(Some(vec![
        row(8, 4, Some(2), frame),
        row(7, 4, Some(1), frame),
    ]));
    let admitted = evidence.window_binding_cg(7).expect("lower window");
    let post = view(false, true, 4, 7, None, Some((10.0, 10.0)));
    assert_eq!(
        evidence.prove_delivery(&admitted, &post),
        Err(DeliveryFault::NotDelivered)
    );
    let mut menu = row(9, 4, Some(3), frame);
    menu.layer = 25;
    facts.set_windows(Some(vec![menu, row(7, 4, Some(1), frame)]));
    let admitted = evidence.window_binding_cg(7).expect("still the app window");
    assert_eq!(
        evidence.prove_delivery(&admitted, &post),
        Err(DeliveryFault::NotDelivered)
    );
}

#[test]
fn prove_post_out_of_bounds_refuses() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_windows(Some(vec![row(7, 4, Some(1), (0.0, 0.0, 40.0, 40.0))]));
    let admitted = evidence.window_binding_cg(7).expect("window");
    assert_eq!(
        evidence.prove_delivery(
            &admitted,
            &view(false, true, 4, 7, None, Some((40.0, 10.0)))
        ),
        Err(DeliveryFault::NotDelivered)
    );
    assert_eq!(
        evidence.prove_delivery(
            &admitted,
            &view(false, true, 4, 7, Some((f64::NAN, 1.0)), None)
        ),
        Err(DeliveryFault::NotDelivered)
    );
}

#[test]
fn prove_post_foreground_window_or_owner_mismatch_refuses() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_process(5, 200, None);
    let frame = (0.0, 0.0, 40.0, 40.0);
    facts.set_windows(Some(vec![
        row(8, 4, Some(2), frame),
        row(7, 4, Some(1), frame),
    ]));
    let admitted = evidence.window_binding_cg(7).expect("target");
    assert_eq!(
        evidence.prove_delivery(&admitted, &view(true, false, 4, 7, None, None)),
        Err(DeliveryFault::NotDelivered)
    );
    assert_eq!(
        evidence.prove_delivery(&admitted, &view(false, false, 5, 7, None, None)),
        Err(DeliveryFault::NotDelivered)
    );
}

#[test]
fn prove_post_unreadable_list_refuses() {
    let (facts, evidence) = facts_for(4, 100);
    facts.set_windows(Some(vec![row(7, 4, Some(1), (0.0, 0.0, 40.0, 40.0))]));
    let admitted = evidence.window_binding_cg(7).expect("window");
    facts.set_windows(None);
    assert_eq!(
        evidence.prove_delivery(&admitted, &view(false, true, 4, 7, Some((1.0, 1.0)), None)),
        Err(DeliveryFault::NotDelivered)
    );
}

#[test]
fn ax_window_capability_missing_symbol_is_not_available() {
    assert_eq!(
        ax_window_capability(false, false, true),
        PrivateApiStatus::Disabled
    );
    assert_eq!(
        ax_window_capability(true, true, true),
        PrivateApiStatus::Disabled
    );
    assert_eq!(
        ax_window_capability(true, false, false),
        PrivateApiStatus::Missing
    );
    assert_eq!(
        ax_window_capability(true, false, true),
        PrivateApiStatus::Available
    );
}

struct Refuse;

impl PostGate for Refuse {
    fn before_post(&self, _event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
        Err(PostFault::NotSent)
    }
}

struct AllowOnce(AtomicU32);

impl PostGate for AllowOnce {
    fn before_post(&self, _event: &SyntheticEvent) -> std::result::Result<(), PostFault> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(())
        } else {
            Err(PostFault::NotSent)
        }
    }
}

fn editor_target() -> InputTarget {
    InputTarget {
        pid: 4,
        window_id: 7,
        bundle_id: "com.example.Editor".into(),
        app_name: "Editor".into(),
        origin_x: 0.0,
        origin_y: 0.0,
        width_px: 100.0,
        height_px: 100.0,
        scale: 1.0,
    }
}

fn click(win: WindowRef) -> DriverAction {
    DriverAction::PointerClick {
        win,
        at: WindowPoint { x: 10.0, y: 10.0 },
        button: Button::Left,
        count: 1,
        route: PointerRoute::TargetedOnly,
    }
}

fn sample_window() -> WindowRef {
    WindowRef {
        app: AppRef {
            info: AppInfo {
                id: "com.example.Editor".into(),
                name: "Editor".into(),
                pid: Some(4),
                identity: None,
            },
        },
        id: "cg:7:4".into(),
    }
}

fn downs(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, Call::Post(event) if event.kind == EventKind::MouseDown))
        .count()
}

#[tokio::test]
async fn deliver_unproved_window_posts_nothing() {
    let events = Arc::new(FakeEvents::ready());
    events.arm_post_gate(Some(Arc::new(Refuse)));
    let error = deliver(
        Arc::clone(&events) as Arc<dyn EventApi>,
        Arc::new(InputLedger::new()),
        &editor_target(),
        &click(sample_window()),
        &Cancel::default(),
        Duration::from_secs(2),
    )
    .await
    .expect_err("refused");
    assert_eq!(
        error,
        DriverError::Native {
            message: "event was not posted".into(),
            maybe_executed: false,
        }
    );
    assert_eq!(downs(&events.calls()), 0);
    assert!(events
        .calls()
        .iter()
        .all(|call| !matches!(call, Call::Post(_))));
}

#[tokio::test]
async fn deliver_second_event_refusal_is_unknown_and_does_not_replay() {
    let events = Arc::new(FakeEvents::ready());
    events.arm_post_gate(Some(Arc::new(AllowOnce(AtomicU32::new(0)))));
    let error = deliver(
        Arc::clone(&events) as Arc<dyn EventApi>,
        Arc::new(InputLedger::new()),
        &editor_target(),
        &click(sample_window()),
        &Cancel::default(),
        Duration::from_secs(2),
    )
    .await
    .expect_err("second event");
    assert_eq!(
        error,
        DriverError::Native {
            message: "event was not posted".into(),
            maybe_executed: true,
        }
    );
    assert_eq!(downs(&events.calls()), 1);
}

async fn editor_session() -> (Arc<FakeEvents>, MacosDriver, WindowRef) {
    let api = Arc::new(FakeMacApi::default());
    api.add_running(
        4,
        &signed_facts("com.example.Editor", "/Applications/Editor.app"),
    );
    api.update(|state| {
        state.frontmost = Some(11);
        state
            .cg_windows
            .push(cg_window(7, 4, "Editor", rect(0.0, 0.0, 80.0, 40.0)));
    });
    let mut driver = MacosDriver::with_api(Arc::clone(&api) as Arc<dyn MacApi>)
        .await
        .expect("driver");
    let app = driver
        .resolve_app(&AppQuery::from("Editor"))
        .await
        .expect("app");
    let windows = driver.list_windows(Some(&app)).await.expect("windows");
    let win = WindowRef {
        app,
        id: windows[0].id.clone(),
    };
    let events = Arc::new(FakeEvents::ready());
    driver.inject_events(Arc::clone(&events) as Arc<dyn EventApi>);
    (events, driver, win)
}

#[tokio::test]
async fn closed_evidence_app_instance_is_none_until_facts_are_installed() {
    let (events, driver, win) = editor_session().await;
    let app = win.app.clone();
    assert!(driver.app_instance(&app).is_none());
    let action = DriverAction::PointerClick {
        win: win.clone(),
        at: WindowPoint { x: 4.0, y: 6.0 },
        button: Button::Left,
        count: 1,
        route: PointerRoute::TargetedOnly,
    };
    driver
        .perform(&action, &Cancel::default())
        .await
        .expect("closed evidence still posts");
    assert!(downs(&events.calls()) >= 1);

    let (events, driver, win) = editor_session().await;
    driver.install_os_facts(Arc::new(FakeOsFacts::new()));
    let error = driver
        .perform(
            &DriverAction::PointerClick {
                win: win.clone(),
                at: WindowPoint { x: 4.0, y: 6.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .expect_err("unproved");
    assert_eq!(error, not_proved());
    assert_eq!(downs(&events.calls()), 0);

    let (events, driver, win) = editor_session().await;
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot");
    facts.set_process(4, 1_700_000_000_000_000, Some("/bin/editor"));
    facts.set_windows(Some(vec![row(7, 4, Some(1), (0.0, 0.0, 80.0, 40.0))]));
    driver.install_os_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    let mut spoofed = win.clone();
    spoofed.id = "cg:7:999".into();
    let binding = driver
        .window_binding(&spoofed)
        .expect("owner from the list");
    assert_eq!(binding.native_id(), "7");
    assert_eq!(binding.owner().pid(), 4);
    assert_eq!(binding.owner().start().ticks(), 1_700_000_000_000_000);
    driver
        .perform(
            &DriverAction::PointerClick {
                win,
                at: WindowPoint { x: 4.0, y: 6.0 },
                button: Button::Left,
                count: 1,
                route: PointerRoute::TargetedOnly,
            },
            &Cancel::default(),
        )
        .await
        .expect("proved window posts");
    assert!(downs(&events.calls()) >= 1);
}

#[cfg(target_os = "macos")]
#[test]
fn own_process_start_is_stable_and_plausible() {
    let pid = std::process::id();
    let facts = super::live::LiveOsFacts::process_only();
    let first = facts.process_start_us(pid).expect("start");
    let second = facts.process_start_us(pid).expect("start again");
    assert_eq!(first, second);
    assert!(first > 1_000_000_000_000_000);
    assert!(first < 10_000_000_000_000_000);
    let boot = facts.boot_identity().expect("boot");
    assert!(!boot.is_empty());
    assert!(boot.len() <= 256);
    let path = facts.executable_path(pid).expect("path");
    assert!(path.starts_with('/'));
}

/// Real window proof. Run later in a Tart guest:
/// `GCU_DESKTOP=1 cargo test -p gcu-driver-macos live_scope_window_proof -- --ignored --exact`.
/// Do not run this on the shared mac-mini.
#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "not_run: Tart guest with GCU_DESKTOP=1; do not run on the shared mac-mini"]
async fn live_scope_window_proof() {
    assert_eq!(
        std::env::var("GCU_DESKTOP").as_deref(),
        Ok("1"),
        "explicit GCU_DESKTOP=1 required"
    );
    let driver = MacosDriver::new().await.expect("live driver");
    let listed = driver.list_windows(None).await.expect("window list");
    assert!(!listed.is_empty(), "desktop window list must be non-empty");
    let placeholder = AppInfo {
        id: "live".into(),
        name: "live".into(),
        pid: None,
        identity: None,
    };
    let mut proved = 0u32;
    for info in &listed {
        let win = WindowRef {
            app: AppRef {
                info: placeholder.clone(),
            },
            id: info.id.clone(),
        };
        let first = driver.window_binding(&win);
        let second = driver.window_binding(&win);
        assert_eq!(first, second);
        if super::spi::ax_window_status() != PrivateApiStatus::Available {
            assert!(first.is_none());
            continue;
        }
        let Some(binding) = first else {
            continue;
        };
        assert!(binding.native_id().chars().all(|ch| ch.is_ascii_digit()));
        let cg: u32 = binding.native_id().parse().expect("decimal cg id");
        let post = view(
            false,
            true,
            i32::try_from(binding.owner().pid()).unwrap_or(-1),
            cg,
            Some((1.0e12, 1.0e12)),
            None,
        );
        assert_eq!(
            driver.evidence.prove_delivery(&binding, &post),
            Err(DeliveryFault::NotDelivered)
        );
        proved += 1;
    }
    if super::spi::ax_window_status() == PrivateApiStatus::Available {
        assert!(
            proved > 0,
            "an available private window symbol must prove a window"
        );
    }
}
