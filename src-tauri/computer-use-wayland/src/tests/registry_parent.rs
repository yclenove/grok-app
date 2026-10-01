//! Deterministic ownership barriers plus actual private portal transport.
use super::adapter::FixturePolicy;
use super::authorization::broker;
use super::registry::{closed, options, ready};
use super::*;
use crate::registry::SelectionOwner;
use grok_computer_use_core::{
    adapter::ComputerUseAdapter,
    native_parent::NativeParent,
    session_grants::{AuthorizationTicket, SessionGrants},
};
use std::sync::atomic::{AtomicBool, Ordering};

struct ControlledParent {
    ticket: AuthorizationTicket,
    ready: AtomicBool,
    handle_requested: AtomicBool,
    revoked: AtomicBool,
    closed: AtomicBool,
}
impl ControlledParent {
    fn new(ticket: &AuthorizationTicket, ready: bool) -> Arc<Self> {
        Arc::new(Self {
            ticket: ticket.clone(),
            ready: AtomicBool::new(ready),
            handle_requested: AtomicBool::new(false),
            revoked: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }
    fn release(&self) {
        self.revoked.store(true, Ordering::SeqCst);
        self.closed.store(true, Ordering::SeqCst);
    }
}
struct ControlledLease(Arc<ControlledParent>);
impl std::ops::Deref for ControlledLease {
    type Target = ControlledParent;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl NativeParent for ControlledLease {
    fn authorization(&self) -> &AuthorizationTicket {
        &self.ticket
    }
    fn handle(&self) -> Result<Option<String>, String> {
        self.handle_requested.store(true, Ordering::SeqCst);
        if self.is_revoked() {
            return Err("controlled parent revoked".into());
        }
        Ok(self
            .ready
            .load(Ordering::SeqCst)
            .then(|| "wayland:owned-controlled-export".into()))
    }
    fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::SeqCst) || self.ticket.is_preparation_cancelled()
    }
    fn request_close(&self) {
        self.revoked.store(true, Ordering::SeqCst);
    }
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

async fn until(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("ownership transition did not occur");
}

fn owned_options(ticket: &AuthorizationTicket) -> PortalOptions {
    let mut value = options(&ticket.run_id);
    value.parent_window.clear();
    value
}

fn select<P, F>(
    registry: &PortalRegistry,
    fixture: &Fixture,
    ticket: &AuthorizationTicket,
    options: PortalOptions,
    prepare: P,
) -> PortalSelection
where
    P: FnOnce() -> F + Send + 'static,
    F: std::future::Future<Output = Result<Box<dyn NativeParent>, String>> + Send + 'static,
{
    let address = fixture.bus.address.clone();
    registry
        .select_owned(
            options,
            SelectionOwner {
                generation: 1,
                target_generation: 1,
                preparation: Some(ticket.clone()),
                parent: Some(Box::new(move || Box::pin(prepare()))),
                policy: None,
            },
            move |o| PortalSession::on_test_bus(o, address),
        )
        .unwrap()
}

async fn delayed_queue(deadline: bool) {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let parent = ControlledParent::new(&ticket, true);
    let (entered_tx, entered) = tokio::sync::oneshot::channel();
    let (deliver, delivery) = tokio::sync::oneshot::channel::<Box<dyn NativeParent>>();
    let weak = Arc::downgrade(&registry);
    let run = ticket.run_id.clone();
    let mut value = owned_options(&ticket);
    if deadline {
        value.timeout = Duration::from_millis(30);
    }
    let selection = select(&registry, &fixture, &ticket, value, move || async move {
        assert!(
            !weak.upgrade().unwrap().is_idle(&run),
            "dispatch before registration"
        );
        entered_tx.send(()).unwrap();
        delivery.await.map_err(|_| "UI queue failed".into())
    });
    entered.await.unwrap();
    if !deadline {
        grants.fence_fail_checked(&broker, &ticket, || {});
    }
    until(|| registry.state(&selection).unwrap() == PortalSelectionState::Closing).await;
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    assert!(!fixture.shared.called("Start"));
    assert!(
        !deliver.is_closed(),
        "cancel/deadline dropped the original UI operation"
    );
    assert!(deliver
        .send(Box::new(ControlledLease(parent.clone())))
        .is_ok());
    until(|| parent.revoked.load(Ordering::SeqCst)).await;
    assert!(
        !registry.is_idle(&ticket.run_id),
        "native export not released yet"
    );
    assert!(registry.forget(&selection).is_err());
    parent.release();
    closed(&registry, &selection).await;
    assert!(registry.is_idle(&ticket.run_id));
    assert!(!fixture.shared.called("Start"));
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_retains_queued_export_and_joins_late_native_owner() {
    delayed_queue(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deadline_retains_queued_export_without_false_idle_or_late_consent() {
    delayed_queue(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_revocation_retains_queued_export_until_original_owner_joins() {
    let fixture = Fixture::new(Mode::Normal).await;
    let policy = Arc::new(FixturePolicy::default());
    let registry = Arc::new(PortalRegistry::new(policy.clone()));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "policy-queue", None).unwrap();
    let parent = ControlledParent::new(&ticket, true);
    let (entered_tx, entered) = tokio::sync::oneshot::channel();
    let (deliver, delivery) = tokio::sync::oneshot::channel::<Box<dyn NativeParent>>();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move {
            entered_tx.send(()).unwrap();
            delivery.await.map_err(|_| "UI queue failed".into())
        },
    );
    entered.await.unwrap();
    policy.deny.store(true, Ordering::SeqCst);
    until(|| registry.state(&selection).unwrap() == PortalSelectionState::Closing).await;
    assert!(
        !deliver.is_closed(),
        "policy loss dropped original UI future"
    );
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    policy.deny.store(false, Ordering::SeqCst);
    assert!(deliver
        .send(Box::new(ControlledLease(parent.clone())))
        .is_ok());
    until(|| parent.revoked.load(Ordering::SeqCst)).await;
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(!fixture.shared.called("Start"));
    parent.release();
    closed(&registry, &selection).await;
    assert!(!fixture.shared.called("Start"));
    assert!(
        matches!(registry.state(&selection).unwrap(), PortalSelectionState::Closed {
        reason: Some(reason)
    } if reason.contains("Host policy revoked"))
    );
    registry.forget(&selection).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_export_before_handle_arrives_joins_original_ui_owner() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let parent = ControlledParent::new(&ticket, false);
    let send = parent.clone();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move { Ok(Box::new(ControlledLease(send)) as Box<dyn NativeParent>) },
    );
    until(|| parent.handle_requested.load(Ordering::SeqCst)).await;
    registry.cancel(&selection).unwrap();
    assert!(parent.revoked.load(Ordering::SeqCst));
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    parent.release();
    closed(&registry, &selection).await;
    assert!(!fixture.shared.called("Start"));
    assert!(registry.is_idle(&ticket.run_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_preparation_with_equal_metadata_is_retained_for_cleanup_not_consent() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let mut foreign = grants.begin(&broker, "other", None).unwrap();
    foreign.session = ticket.session.clone();
    foreign.run_id = ticket.run_id.clone();
    foreign.attempt_id = ticket.attempt_id.clone();
    foreign.selector_revision = ticket.selector_revision;
    foreign.generation = ticket.generation;
    let parent = ControlledParent::new(&foreign, true);
    let send = parent.clone();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move { Ok(Box::new(ControlledLease(send)) as Box<dyn NativeParent>) },
    );
    until(|| parent.revoked.load(Ordering::SeqCst)).await;
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    assert!(!fixture.shared.called("Start"));
    parent.release();
    closed(&registry, &selection).await;
    assert!(
        matches!(registry.state(&selection).unwrap(), PortalSelectionState::Closed { reason: Some(reason) } if reason.contains("different authorization"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_grant_loses_input_synchronously_when_native_parent_is_lost() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let parent = ControlledParent::new(&ticket, true);
    let send = parent.clone();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move { Ok(Box::new(ControlledLease(send)) as Box<dyn NativeParent>) },
    );
    let target = ready(&registry, &selection).await;
    let retained_adapter = registry.entry(&ticket.run_id).unwrap().adapter().unwrap();
    broker.authorize_target(&ticket.run_id, &target).unwrap();
    grants.publish(&broker, &ticket, true, || {}).unwrap();
    assert!(registry.input_available());
    parent.revoked.store(true, Ordering::SeqCst);
    assert!(!registry.input_available());
    assert!(
        !retained_adapter.input_available(),
        "cached adapter bypassed native parent"
    );
    assert!(!registry.target_alive(&target));
    assert!(!registry.is_idle(&ticket.run_id));
    assert!(registry.forget(&selection).is_err());
    until(|| {
        fixture
            .shared
            .sessions
            .lock()
            .unwrap()
            .iter()
            .all(|s| fixture.shared.called(&format!("Session.Close:{s}")))
    })
    .await;
    assert!(
        !registry.is_idle(&ticket.run_id),
        "portal Closed does not release parent"
    );
    parent.release();
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn portal_denial_cannot_report_closed_before_parent_release() {
    let fixture = Fixture::new(Mode::Denied).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let parent = ControlledParent::new(&ticket, true);
    let send = parent.clone();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move { Ok(Box::new(ControlledLease(send)) as Box<dyn NativeParent>) },
    );
    until(|| parent.revoked.load(Ordering::SeqCst)).await;
    assert!(!registry.is_idle(&ticket.run_id));
    assert_eq!(
        registry.state(&selection).unwrap(),
        PortalSelectionState::Closing
    );
    parent.release();
    closed(&registry, &selection).await;
    fixture.assert_sessions_closed();
}

#[test]
fn invalid_owned_selection_never_dispatches_ui_export() {
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    for case in 0..5 {
        let mut value = owned_options(&ticket);
        match case {
            0 => value.parent_window = "wayland:model-supplied".into(),
            1 => value.source = SourceKind::Window,
            2 => value.run_id = "foreign".into(),
            3 => value.timeout = Duration::ZERO,
            _ => {}
        }
        assert!(registry
            .select_parented_for_authorization(
                value,
                if case == 4 { 0 } else { 1 },
                1,
                &ticket,
                || async { panic!("invalid request dispatched UI export") }
            )
            .is_err());
    }
    grants.fence_fail_checked(&broker, &ticket, || {});
    assert!(registry
        .select_parented_for_authorization(owned_options(&ticket), 1, 1, &ticket, || async {
            panic!("cancelled ticket dispatched UI export")
        })
        .is_err());
    assert!(registry.all_entries().is_empty());
}

#[tokio::test]
async fn lost_ui_reply_is_unknown_completion_not_a_joined_error() {
    let (send, receive) = tokio::sync::oneshot::channel::<Result<ControlledLease, String>>();
    drop(send);
    let original = grok_computer_use_core::native_parent::retain_parent_dispatch(receive);
    tokio::pin!(original);
    for _ in 0..2 {
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut original)
                .await
                .is_err()
        );
    }
    // This unit case created no native owner. The production registry retains
    // the same unresolved future, rather than dropping it as this test does.
}

#[tokio::test]
async fn known_pre_export_failure_is_delivered_without_inventing_a_parent() {
    let (send, receive) = tokio::sync::oneshot::channel::<Result<ControlledLease, String>>();
    assert!(send.send(Err("rejected before export".into())).is_ok());
    let result = grok_computer_use_core::native_parent::retain_parent_dispatch(receive).await;
    assert!(matches!(result, Err(reason) if reason == "rejected before export"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_loss_while_ui_prepares_prevents_portal_start_and_joins_parent() {
    let fixture = Fixture::new(Mode::Normal).await;
    let policy = Arc::new(FixturePolicy::default());
    let registry = Arc::new(PortalRegistry::new(policy.clone()));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let parent = ControlledParent::new(&ticket, false);
    let send = parent.clone();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move { Ok(Box::new(ControlledLease(send)) as Box<dyn NativeParent>) },
    );
    until(|| parent.handle_requested.load(Ordering::SeqCst)).await;
    policy.deny.store(true, Ordering::SeqCst);
    parent.ready.store(true, Ordering::SeqCst);
    until(|| parent.revoked.load(Ordering::SeqCst)).await;
    assert!(!fixture.shared.called("Start"));
    assert!(!registry.is_idle(&ticket.run_id));
    parent.release();
    closed(&registry, &selection).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_registry_retains_the_original_ui_release_barrier() {
    let fixture = Fixture::new(Mode::Normal).await;
    let registry = Arc::new(PortalRegistry::new(Arc::new(FixturePolicy::default())));
    let broker = broker(&registry);
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let parent = ControlledParent::new(&ticket, true);
    let send = parent.clone();
    let selection = select(
        &registry,
        &fixture,
        &ticket,
        owned_options(&ticket),
        move || async move { Ok(Box::new(ControlledLease(send)) as Box<dyn NativeParent>) },
    );
    ready(&registry, &selection).await;
    let original = registry.entry(&ticket.run_id).unwrap();
    drop(grants);
    drop(broker);
    drop(registry);
    until(|| parent.revoked.load(Ordering::SeqCst)).await;
    assert!(!original.idle());
    parent.release();
    until(|| original.idle()).await;
    fixture.assert_sessions_closed();
    fixture.shared.assert_peers_closed(2);
}
