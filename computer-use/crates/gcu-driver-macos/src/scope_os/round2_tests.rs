//! Round-2 schedules for the admitted-binding rule. No window server.

use std::sync::Arc;

use gcu_core::driver::{
    AppQuery, AppRef, Button, Cancel, Driver, DriverAction, DriverError, KeyRoute, PointerRoute,
    WindowRef,
};
use gcu_core::model::{NativeHandle, WindowPoint};
use gcu_core::scope::{
    BootIdentity, InstanceEvidence, OsStartTime, ProcessInstance, WindowBinding,
};
use gcu_protocol::capabilities::{ForegroundCapability, PrivateApiStatus};
use gcu_protocol::KeyChord;

use super::evidence::{not_proved, Evidence, SCOPED_ROUTE_NOTE};
use super::facts::{boot_identity_from, ElementFact, FakeOsFacts, OsFacts, OsWindow};
use super::spi::ax_window_capability;
use crate::api::MacApi;
use crate::ax::{AxHandle, AxIdentity};
use crate::driver::MacosDriver;
use crate::input::{Call, EventApi, EventKind, FakeEvents};
use crate::testing::{cg_window, rect, signed_facts, FakeMacApi};

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

fn click(win: WindowRef) -> DriverAction {
    DriverAction::PointerClick {
        win,
        at: WindowPoint { x: 4.0, y: 6.0 },
        button: Button::Left,
        count: 1,
        route: PointerRoute::TargetedOnly,
    }
}

fn posts(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, Call::Post(_)))
        .count()
}

fn key_downs(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, Call::Post(event) if event.kind == EventKind::KeyDown))
        .count()
}

fn activates(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, Call::Activate(_)))
        .count()
}

fn raises(calls: &[Call]) -> Vec<(i32, u32)> {
    calls
        .iter()
        .filter_map(|call| match call {
            Call::Raise { pid, window_id } => Some((*pid, *window_id)),
            _ => None,
        })
        .collect()
}

fn assert_nothing_executed(err: &DriverError) {
    match err {
        DriverError::Native { maybe_executed, .. } => assert!(!*maybe_executed),
        other => panic!("expected an unexecuted refusal, got {other:?}"),
    }
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

fn install_window(driver: &MacosDriver, token: u64) -> (Arc<FakeOsFacts>, WindowBinding) {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 1_700_000_000_000_000, Some("/bin/editor"));
    facts.set_windows(Some(vec![row(7, 4, Some(token), (0.0, 0.0, 80.0, 40.0))]));
    driver.install_os_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    let binding = driver
        .window_binding(&WindowRef {
            app: AppRef {
                info: gcu_protocol::AppInfo {
                    id: "x".into(),
                    name: "x".into(),
                    pid: None,
                    identity: None,
                },
            },
            id: "cg:7:4".into(),
        })
        .expect("admitted");
    (facts, binding)
}

#[tokio::test]
async fn rebound_window_after_admission_posts_nothing() {
    let (events, driver, win) = editor_session().await;
    let (facts, admitted) = install_window(&driver, 11);
    driver.evidence.pin(Some(admitted));
    facts.set_windows(Some(vec![row(7, 4, Some(99), (0.0, 0.0, 80.0, 40.0))]));
    let err = driver
        .perform(&click(win), &Cancel::default())
        .await
        .expect_err("replacement window");
    assert_eq!(
        err,
        DriverError::Native {
            message: "event was not posted".into(),
            maybe_executed: false,
        }
    );
    assert_eq!(posts(&events.calls()), 0);
}

#[test]
fn token_refresh_cg_id_reuse_refuses_binding() {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 100, Some("/bin/app"));
    facts.set_windows(Some(vec![row(7, 4, Some(11), (0.0, 0.0, 40.0, 40.0))]));
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    assert!(evidence.window_binding_cg(7).is_some());
    facts.arm_token_cg_reuse();
    assert!(!facts.observation_lost());
    assert_eq!(facts.process_start_us(4), Some(100));
    let rows = facts.windows().expect("list still readable");
    assert_eq!(rows[0].cg_id, 7);
    assert_eq!(rows[0].token, Some(11));
    assert!(facts.snapshot().is_none());
    assert!(evidence.window_binding_cg(7).is_none());
}

#[test]
fn same_frame_floating_panel_writes_nothing() {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 100, Some("/bin/app"));
    let frame = (0.0, 0.0, 80.0, 40.0);
    let mut panel = row(8, 4, Some(2), frame);
    panel.layer = 3;
    facts.set_windows(Some(vec![panel, row(7, 4, Some(1), frame)]));
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    let admitted = evidence.window_binding_cg(7).expect("window A");
    evidence.pin(Some(admitted.clone()));
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 4,
            cg_id: Some(8),
        },
    );
    let mut writes = 0u32;
    let err = evidence
        .commit_ax_write(&admitted, 4, 3, || writes += 1)
        .expect_err("panel");
    assert_nothing_executed(&err);
    assert_eq!(writes, 0);
}

#[test]
fn element_moved_before_append_writes_nothing() {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 100, Some("/bin/app"));
    facts.set_windows(Some(vec![row(7, 4, Some(1), (0.0, 0.0, 40.0, 40.0))]));
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 4,
            cg_id: Some(7),
        },
    );
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    let admitted = evidence.window_binding_cg(7).expect("window");
    evidence.pin(Some(admitted.clone()));
    let moved = Arc::clone(&facts);
    evidence.set_pre_sink_barrier(move || {
        moved.set_element(
            4,
            3,
            ElementFact {
                pid: 4,
                cg_id: Some(42),
            },
        );
    });
    let mut writes = 0u32;
    let err = evidence
        .commit_ax_write(&admitted, 4, 3, || writes += 1)
        .expect_err("unregistered window");
    assert_nothing_executed(&err);
    assert_eq!(writes, 0);
}

#[test]
fn reparented_element_does_not_replace_admitted_window() {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 100, Some("/bin/app"));
    let frame = (0.0, 0.0, 40.0, 40.0);
    facts.set_windows(Some(vec![
        row(7, 4, Some(1), frame),
        row(8, 4, Some(2), frame),
    ]));
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    let admitted = evidence.window_binding_cg(7).expect("A");
    evidence.pin(Some(admitted.clone()));
    facts.set_element(
        4,
        3,
        ElementFact {
            pid: 4,
            cg_id: Some(8),
        },
    );
    let handle = NativeHandle::new(AxHandle {
        pid: 4,
        id: 3,
        identity: AxIdentity::default(),
    });
    assert!(evidence.mutation_window(&handle).is_err());
    assert_eq!(evidence.pinned().expect("pin stays"), admitted);
}

#[tokio::test]
async fn scoped_hid_keys_with_panel_above_post_nothing() {
    let (events, driver, win) = editor_session().await;
    let (facts, admitted) = install_window(&driver, 11);
    let frame = (0.0, 0.0, 80.0, 40.0);
    let mut panel = row(8, 4, Some(22), frame);
    panel.layer = 3;
    facts.set_windows(Some(vec![panel, row(7, 4, Some(11), frame)]));
    driver.evidence.pin(Some(admitted));
    let mut probe = events.probe();
    probe.field_51 = false;
    probe.window_location_symbol = false;
    probe.arrival_verified = false;
    events.set_probe(probe);
    let err = driver
        .perform(
            &DriverAction::PressKeys {
                win,
                chord: KeyChord::parse("a").expect("chord"),
                route: KeyRoute::AllowForeground,
            },
            &Cancel::default(),
        )
        .await
        .expect_err("hid keyboard");
    assert!(matches!(err, DriverError::CapabilityUnavailable));
    assert_eq!(key_downs(&events.calls()), 0);
    assert_eq!(activates(&events.calls()), 0);
    assert_eq!(posts(&events.calls()), 0);
}

#[tokio::test]
async fn scoped_activate_other_window_raises_only_admitted() {
    let (events, driver, win) = editor_session().await;
    let (facts, _) = install_window(&driver, 11);
    let frame = (0.0, 0.0, 80.0, 40.0);
    facts.set_windows(Some(vec![
        row(7, 4, Some(11), frame),
        row(8, 4, Some(22), (90.0, 0.0, 40.0, 40.0)),
    ]));
    let admitted = driver
        .window_binding(&WindowRef {
            app: win.app.clone(),
            id: "cg:7:4".into(),
        })
        .expect("A");
    driver.evidence.pin(Some(admitted));
    driver
        .perform(&DriverAction::Activate { win }, &Cancel::default())
        .await
        .expect("confined raise");
    assert_eq!(activates(&events.calls()), 0);
    assert_eq!(raises(&events.calls()), vec![(4, 7)]);
}

#[tokio::test]
async fn scoped_activate_pid_reuse_activates_nothing() {
    let (events, driver, win) = editor_session().await;
    let (facts, admitted) = install_window(&driver, 11);
    driver.evidence.pin(Some(admitted));
    let replaced = Arc::clone(&facts);
    driver.evidence.set_pre_sink_barrier(move || {
        replaced.set_process(4, 1_800_000_000_000_000, Some("/bin/editor"));
    });
    let err = driver
        .perform(&DriverAction::Activate { win }, &Cancel::default())
        .await
        .expect_err("pid reuse");
    assert_eq!(err, not_proved());
    assert_nothing_executed(&err);
    assert_eq!(activates(&events.calls()), 0);
    assert!(raises(&events.calls()).is_empty());
}

#[tokio::test]
async fn focus_stolen_after_proof_posts_nothing() {
    let (events, driver, win) = editor_session().await;
    let (facts, admitted) = install_window(&driver, 11);
    driver.evidence.pin(Some(admitted));
    let stolen = Arc::clone(&facts);
    driver.evidence.set_pre_sink_barrier(move || {
        stolen.set_process(4, 1_800_000_000_000_000, Some("/bin/editor"));
    });
    let err = driver
        .perform(&click(win), &Cancel::default())
        .await
        .expect_err("stolen owner");
    assert_eq!(
        err,
        DriverError::Native {
            message: "event was not posted".into(),
            maybe_executed: false,
        }
    );
    assert_eq!(posts(&events.calls()), 0);
}

#[test]
fn boot_time_without_uuid_is_not_identity() {
    assert!(boot_identity_from(None, Some("1700000000.20")).is_none());
    assert!(boot_identity_from(Some(""), Some("1700000000.20")).is_none());
    assert_eq!(
        boot_identity_from(Some("uuid-a"), Some("1.2")).as_deref(),
        Some("uuid-a")
    );
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot_time("1700000000.20");
    facts.set_process(4, 100, Some("/bin/app"));
    facts.set_windows(Some(vec![row(7, 4, Some(1), (0.0, 0.0, 40.0, 40.0))]));
    let evidence = Evidence::from_facts(Arc::clone(&facts) as Arc<dyn OsFacts>);
    assert!(evidence.app_pid(4).is_none());
    assert!(evidence.window_binding_cg(7).is_none());
}

#[test]
fn fresh_tables_do_not_reuse_incarnation_one() {
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 100, Some("/bin/app"));
    facts.set_windows(Some(vec![row(7, 4, Some(1), (0.0, 0.0, 40.0, 40.0))]));
    let shared = Arc::clone(&facts) as Arc<dyn OsFacts>;
    let first = Evidence::from_facts(Arc::clone(&shared));
    let second = Evidence::from_facts(shared);
    let left = first.window_binding_cg(7).expect("first table");
    let right = second.window_binding_cg(7).expect("second table");
    assert_ne!(left, right);
    assert_ne!(left.incarnation(), 1);
    assert_ne!(right.incarnation(), 1);
    assert_ne!(left.incarnation(), right.incarnation());
    assert_eq!(left.native_id(), right.native_id());
    assert_eq!(left.owner(), right.owner());
}

#[test]
fn distinct_boot_descriptors_are_not_equal() {
    let owner = |boot: &str| {
        ProcessInstance::new(
            4,
            OsStartTime::new(100).expect("start"),
            BootIdentity::new(boot).expect("boot"),
        )
        .expect("owner")
    };
    let left = WindowBinding::new("7", 5, owner("boot-a")).expect("left");
    let right = WindowBinding::new("7", 5, owner("boot-b")).expect("right");
    assert_ne!(left, right);
    assert_eq!(left.incarnation(), right.incarnation());
    assert_eq!(left.native_id(), right.native_id());
}

#[tokio::test]
async fn live_adapter_without_scope_posts_when_spi_symbol_is_missing() {
    assert_eq!(
        ax_window_capability(true, false, false),
        PrivateApiStatus::Missing
    );
    let (events, mut driver, win) = editor_session().await;
    let facts = Arc::new(FakeOsFacts::new());
    facts.set_boot("boot-uuid");
    facts.set_process(4, 1_700_000_000_000_000, Some("/bin/editor"));
    facts.set_windows(Some(vec![row(7, 4, None, (0.0, 0.0, 80.0, 40.0))]));
    driver.evidence = Evidence::live_adapter(Arc::clone(&facts) as Arc<dyn OsFacts>);
    driver
        .perform(&click(win), &Cancel::default())
        .await
        .expect("unscoped input");
    assert!(posts(&events.calls()) >= 1);
}

#[tokio::test]
async fn scoped_capabilities_mark_global_hid_unavailable() {
    let (_events, driver, _win) = editor_session().await;
    let unscoped = driver.capabilities().clone();
    let scoped = InstanceEvidence::scope_capabilities(&driver, &unscoped);
    assert_eq!(
        scoped.delivery.foreground,
        ForegroundCapability::Unavailable
    );
    assert_eq!(
        driver.capabilities().delivery.foreground,
        unscoped.delivery.foreground
    );
    assert_eq!(driver.scoped_doctor_note(), Some(SCOPED_ROUTE_NOTE));
    assert!(!SCOPED_ROUTE_NOTE.contains("CANARY"));
    let report = driver.doctor().await;
    assert!(report
        .issues
        .iter()
        .all(|issue| !issue.contains("scoped mode")));
}
