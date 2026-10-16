//! Semantic delivery traverses the real subprocess codec and native driver dispatch.
use super::*;
use gcu_core::{
    driver::{AppQuery, Delivery, Dir, WindowRef},
    model::{NodeAction, TextRange},
};

pub(super) async fn window(driver: &WindowsDriver) -> WindowRef {
    let app = driver
        .resolve_app(&AppQuery("Editor".into()))
        .await
        .unwrap();
    let id = driver.key_window(&app).await.unwrap().id;
    WindowRef { app, id }
}

#[tokio::test]
async fn semantic_actions_worker_boundary_delivers_all_variants_once() {
    let f = Fixture::new();
    let driver = WindowsDriver::isolated(f.config("normal")).await.unwrap();
    let win = window(&driver).await;
    let root = driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap()
        .root
        .unwrap();
    for action in [
        DriverAction::Press {
            el: root.handle.clone(),
        },
        DriverAction::SecondaryAction {
            el: root.handle.clone(),
            action: NodeAction::Toggle,
        },
        DriverAction::SetValue {
            el: root.handle.clone(),
            value: "UIA 中🦀".into(),
        },
        DriverAction::SelectText {
            el: root.handle.clone(),
            range: TextRange {
                start: 1,
                length: 2,
            },
        },
        DriverAction::ScrollElement {
            el: root.handle.clone(),
            dir: Dir::Down,
            pages: 1.,
        },
    ] {
        assert_eq!(
            driver.perform(&action, &Cancel::default()).await,
            Ok(Delivery::Accessibility)
        );
    }
    assert_eq!(f.calls(), vec!["mutation"; 5]);
    assert_eq!(f.pids().len(), 1);
    assert_eq!(
        driver
            .perform(
                &DriverAction::SetValue {
                    el: root.children[0].handle.clone(),
                    value: "not writable".into(),
                },
                &Cancel::default(),
            )
            .await,
        Err(DriverError::NotSettable {
            maybe_executed: false
        })
    );
    assert_eq!(
        driver
            .perform(
                &DriverAction::SecondaryAction {
                    el: root.handle,
                    action: NodeAction::Raise,
                },
                &Cancel::default(),
            )
            .await,
        Err(DriverError::ElementNotActionable)
    );
}

#[tokio::test]
async fn semantic_unknown_worker_boundary_preserves_uncertainty_without_retry() {
    let f = Fixture::new();
    let driver = WindowsDriver::isolated(f.config("normal_unknown"))
        .await
        .unwrap();
    let win = window(&driver).await;
    let el = driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap()
        .root
        .unwrap()
        .handle;
    let action = DriverAction::Press { el };
    assert_eq!(
        driver.perform(&action, &Cancel::default()).await,
        Err(DriverError::Native {
            message: "synthetic uncertain provider failure".into(),
            maybe_executed: true,
        })
    );
    assert_eq!(f.calls(), ["mutation"]);
    assert_eq!(f.pids().len(), 1);
    // Only a new explicit call may dispatch again after an unknown result.
    assert_eq!(
        driver.perform(&action, &Cancel::default()).await,
        Ok(Delivery::Accessibility)
    );
    assert_eq!(f.calls(), ["mutation", "mutation"]);
}

#[tokio::test]
async fn native_pre_dispatch_timeout_worker_boundary_preserves_zero_effect_without_retry() {
    let f = Fixture::new();
    let supervisor = Supervisor::start(f.config("native_timeout_before"))
        .await
        .unwrap();
    assert!(matches!(
        supervisor.execute(mutation(), &Cancel::default()).await,
        Err(DriverError::Timeout {
            maybe_executed: false,
        })
    ));
    assert_eq!(f.calls(), ["mutation"]);
    supervisor.shutdown().await.unwrap();
}

#[tokio::test]
async fn set_value_blocked_driver_deadline_recovers_next_snapshot_without_replay() {
    let f = Fixture::new();
    let mut config = f.config("normal_hang");
    config.timeout = Duration::from_secs(10); // Driver deadline must win the race.
    let driver = std::sync::Arc::new(WindowsDriver::isolated(config).await.unwrap());
    let win = window(&driver).await;
    let root = driver
        .snapshot_tree(&win, &Default::default(), &Cancel::default())
        .await
        .unwrap()
        .root
        .unwrap();
    let action = {
        let driver = driver.clone();
        tokio::spawn(async move {
            driver
                .perform(
                    &DriverAction::SetValue {
                        el: root.handle,
                        value: "blocked".into(),
                    },
                    &Cancel::default(),
                )
                .await
        })
    };
    dispatched(&f).await;
    tokio::time::pause();
    tokio::time::advance(gcu_core::driver::DRIVER_DEADLINE).await;
    tokio::time::resume();
    assert!(matches!(
        action.await.unwrap(),
        Err(DriverError::Timeout {
            maybe_executed: true
        })
    ));
    let raw = tokio::time::timeout(
        Duration::from_secs(10),
        driver.snapshot_tree(&win, &Default::default(), &Cancel::default()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(raw.root.is_some());
    assert!(raw.incomplete.is_none());
    assert_eq!(f.calls(), ["mutation"]);
    assert_eq!(f.pids().len(), 2);
    driver.release_all_input().await;
}
