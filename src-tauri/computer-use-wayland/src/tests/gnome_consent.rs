use super::*;
use crate::GnomePolicyActivation;

async fn pending(
    source: &Source,
) -> (
    Arc<dyn PortalInputPolicy>,
    GnomePolicyActivation,
    tokio::sync::oneshot::Sender<()>,
    Owner,
) {
    let (watch, activation) = source.consent_watch().await.unwrap();
    let active = watch.input_policy();
    assert!(!active.input_available());
    let (stop, receive) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(receive));
    ready(&activation.consent_policy()).await;
    assert!(!active.input_available());
    (active, activation, stop, owner)
}
async fn call_after(source: &Source, previous: u32) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while source.helper.calls.load(Ordering::SeqCst) <= previous {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn picker_clicks_never_grant_input_then_same_watch_arms_exactly_once() {
    let source = Source::consent_source().await;
    let (active, activation, stop, owner) = pending(&source).await;
    for _ in 0..5 {
        source.takeover().await;
    }
    tokio::time::sleep(Duration::from_millis(240)).await;
    assert!(activation.consent_ready());
    assert!(!active.input_available());
    let armed = activation.activate().await.unwrap();
    assert!(
        Arc::ptr_eq(&active, &armed),
        "replaced original policy/subscriptions"
    );
    assert!(active.input_available());
    stop.send(()).unwrap();
    owner.await.unwrap().unwrap();
    assert!(!active.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_physical_event_after_consent_permanently_retires_grant() {
    let source = Source::consent_source().await;
    let (active, activation, _stop, owner) = pending(&source).await;
    activation.activate().await.unwrap();
    source.takeover().await;
    assert!(ended(owner).await.contains("new consent"));
    assert!(active.user_input_active());
    assert!(!active.input_available());
    source.change(state()).await;
    assert!(!active.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_reload_version_or_owner_loss_during_picker_remain_fatal() {
    for case in 0..4 {
        let source = Source::consent_source().await;
        let (active, activation, _stop, owner) = pending(&source).await;
        let mut value = state();
        match case {
            0 => {
                value.3 = true;
                source.change(value).await;
                source.change(state()).await;
            }
            1 => {
                value.1 = uuid::Uuid::new_v4().to_string();
                source.change(value).await;
            }
            2 => {
                value.0 = 2;
                source.change(value).await;
            }
            _ => {
                source.shell.server.release_name(SHELL).await.unwrap();
            }
        }
        assert!(!ended(owner).await.is_empty());
        assert!(!activation.consent_ready());
        assert!(activation.activate().await.is_err());
        assert!(!active.input_available());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consent_initialization_lock_pulse_cannot_be_absorbed_as_picker_input() {
    let source = Source::consent_source().await;
    source.helper.pulse.store(true, Ordering::SeqCst);
    let (watch, activation) = source.consent_watch().await.unwrap();
    let (_stop, receive) = tokio::sync::oneshot::channel();
    let owner = tokio::spawn(watch.run_until(receive));
    assert!(!ended(owner).await.is_empty());
    assert!(!activation.consent_ready());
    assert!(activation.activate().await.is_err());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changed_generation_while_activation_snapshot_waits_cannot_be_rebased() {
    let source = Source::consent_source().await;
    let (active, activation, _stop, owner) = pending(&source).await;
    source.helper.hold.store(true, Ordering::SeqCst);
    let previous = source.helper.calls.load(Ordering::SeqCst);
    let arming = tokio::spawn(activation.activate());
    call_after(&source, previous).await;
    source.takeover().await;
    source.helper.hold.store(false, Ordering::SeqCst);
    source.helper.release.notify_waiters();
    assert!(arming.await.unwrap().is_err());
    assert!(!ended(owner).await.is_empty());
    assert!(!active.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_pulse_in_activation_reply_does_not_create_ready_adapter_policy() {
    let source = Source::consent_source().await;
    let (active, activation, _stop, owner) = pending(&source).await;
    source.helper.pulse.store(true, Ordering::SeqCst);
    let arming = activation.activate().await;
    assert!(
        arming.is_err(),
        "activation accepted a lock pulse preceding its snapshot reply"
    );
    assert!(!ended(owner).await.is_empty());
    assert!(!active.input_available());
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropped_or_cancelled_activator_never_arms_even_after_late_reply() {
    for during_reply in [false, true] {
        let source = Source::consent_source().await;
        let (active, activation, _stop, owner) = pending(&source).await;
        if during_reply {
            source.helper.hold.store(true, Ordering::SeqCst);
            let previous = source.helper.calls.load(Ordering::SeqCst);
            let arming = tokio::spawn(activation.activate());
            call_after(&source, previous).await;
            arming.abort();
            assert!(arming.await.err().unwrap().is_cancelled());
            source.helper.hold.store(false, Ordering::SeqCst);
            source.helper.release.notify_waiters();
        } else {
            drop(activation);
        }
        assert!(!ended(owner).await.is_empty());
        assert!(!active.input_available());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn helper_hang_or_fault_during_picker_never_becomes_an_input_grant() {
    for hang in [false, true] {
        let source = Source::consent_source().await;
        let (active, activation, _stop, owner) = pending(&source).await;
        if hang {
            source.helper.hold.store(true, Ordering::SeqCst);
        } else {
            source.helper.fail.store(true, Ordering::SeqCst);
        }
        assert!(!ended(owner).await.is_empty());
        source.helper.hold.store(false, Ordering::SeqCst);
        source.helper.release.notify_waiters();
        assert!(activation.activate().await.is_err());
        assert!(!active.input_available());
    }
}
#[tokio::test]
async fn stalled_pending_reactor_never_revives_when_activation_is_polled() {
    let source = Source::consent_source().await;
    let (active, activation, _stop, owner) = pending(&source).await;
    std::thread::sleep(Duration::from_millis(520));
    assert!(!activation.consent_ready());
    assert!(activation.activate().await.is_err());
    assert!(!ended(owner).await.is_empty());
    assert!(!active.input_available());
}
