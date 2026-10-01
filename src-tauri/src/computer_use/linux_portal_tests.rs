use super::*;
use grok_computer_use_core::{
    adapter::ComputerUseAdapter,
    broker::{BrokerOptions, ComputerUseBroker},
    session_grants::SessionGrants,
};

#[test]
fn native_session_never_uses_display_or_desktop_brand_as_authority() {
    assert!(native_session(Some("wayland"), None));
    assert!(native_session(Some("WAYLAND"), None));
    assert!(native_session(None, Some("wayland-0")));
    assert!(native_session(Some("x11"), Some("wayland-1")));
    assert!(!native_session(Some("x11"), None));
    assert!(!native_session(None, Some("  ")));
    assert!(!native_session(Some("GNOME"), None));
    assert!(!native_session(None, None));
}

struct FixturePolicy;

#[test]
fn preview_factory_and_status_agree_without_enabling_a_native_release_claim() {
    let expected = cfg!(feature = "computer-use-wayland-preview")
        && native_session(
            std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
            std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        );
    assert_eq!(selected(), expected);
    assert_eq!(
        crate::computer_use::desktop_selection_mode(),
        if expected { "portal" } else { "targets" }
    );
    if expected {
        let adapter = crate::computer_use::os_adapter();
        assert_eq!(adapter.backend_id(), "wayland-portal");
        assert!(!adapter.capabilities().native_wayland);
    }
}

impl PortalInputPolicy for FixturePolicy {
    fn input_available(&self) -> bool {
        true
    }
    fn user_input_active(&self) -> bool {
        false
    }
}

// Held parent preparation only: no GTK export, system bus, portal, input or
// desktop access. Exercises the real App guard, registry thread and join path.
async fn held_parent() -> (
    PreparedTarget,
    tokio::sync::oneshot::Sender<()>,
    String,
    SessionGrants,
) {
    let broker = ComputerUseBroker::new(
        Arc::new(crate::computer_use::test_support::CountingAdapter::default()),
        BrokerOptions {
            feature_enabled: true,
            ..BrokerOptions::default()
        },
    );
    // The real App owns SessionGrants for the whole command. Dropping a
    // temporary here correctly cancels its ticket before preparation starts.
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "parent-command-guard", None).unwrap();
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy)));
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, held) = tokio::sync::oneshot::channel();
    let selection = registry
        .select_parented_for_authorization(
            PortalOptions::new(ticket.run_id.clone(), SourceKind::Monitor),
            1,
            1,
            &ticket,
            move || async move {
                started.send(()).unwrap();
                held.await
                    .map_err(|_| "fixture parent sender lost".to_string())?;
                Err("fixture parent created no native export".into())
            },
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    (
        PreparedTarget {
            registry,
            selection,
            target: String::new(),
            committed: false,
        },
        release,
        ticket.run_id,
        grants,
    )
}

async fn forgotten(registry: &PortalRegistry, selection: &PortalSelection) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while registry.state(selection).is_ok() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("exact selection not retired after owner joined");
}

#[tokio::test]
async fn app_dropped_consent_guard_cancels_but_cannot_forget_held_parent() {
    let (guard, release, run, _grants) = held_parent().await;
    let registry = guard.registry.clone();
    let selection = guard.selection.clone();
    drop(guard);
    assert!(matches!(
        registry.state(&selection).unwrap(),
        PortalSelectionState::Closing
    ));
    assert!(!registry.is_idle(&run));
    assert!(registry.forget(&selection).is_err());
    release.send(()).unwrap();
    forgotten(&registry, &selection).await;
    assert!(registry.is_idle(&run));
}

#[tokio::test]
async fn app_committed_guard_does_not_cancel_but_still_retires_after_stop() {
    let (mut guard, release, _, _grants) = held_parent().await;
    let registry = guard.registry.clone();
    let selection = guard.selection.clone();
    // Guard semantics only; this fixture does not claim a native grant/commit.
    guard.commit();
    drop(guard);
    assert!(matches!(
        registry.state(&selection).unwrap(),
        PortalSelectionState::Pending
    ));
    assert!(registry.forget(&selection).is_err());
    registry.cancel(&selection).unwrap();
    release.send(()).unwrap();
    forgotten(&registry, &selection).await;
}

#[tokio::test]
async fn app_command_reads_parent_failure_before_metadata_retirement() {
    let (guard, release, _, _grants) = held_parent().await;
    let registry = guard.registry.clone();
    let selection = guard.selection.clone();
    release.send(()).unwrap();
    let reason = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let PortalSelectionState::Closed { reason } = registry.state(&selection).unwrap() {
                break reason;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(reason
        .unwrap()
        .contains("fixture parent created no native export"));
    assert!(
        registry.state(&selection).is_ok(),
        "live command lost its failure reason"
    );
    drop(guard);
    forgotten(&registry, &selection).await;
}

#[tokio::test]
async fn app_host_session_loss_revokes_without_dropping_original_parent_future() {
    let (guard, release, run, grants) = held_parent().await;
    let registry = guard.registry.clone();
    let selection = guard.selection.clone();
    drop(grants);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(
            registry.state(&selection).unwrap(),
            PortalSelectionState::Closing
        ) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Host session loss did not revoke pending consent");
    assert!(!registry.is_idle(&run));
    assert!(registry.forget(&selection).is_err());
    release.send(()).unwrap();
    drop(guard);
    forgotten(&registry, &selection).await;
    assert!(registry.is_idle(&run));
}
