//! The selection owns one original OS watch through picker, activation and join.
use super::*;
use crate::{GnomeNativePolicyWatch, GnomePolicyActivation};
use std::{future::Future, pin::Pin};
use tokio::sync::oneshot;

type Prepared = (GnomeNativePolicyWatch, GnomePolicyActivation);
pub(crate) type PolicyPreparation = Box<
    dyn FnOnce(
            Arc<dyn PortalInputPolicy>,
        ) -> Pin<Box<dyn Future<Output = Result<Prepared, String>> + Send>>
        + Send,
>;

pub(super) struct Monitor {
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), String>>,
    activation: Option<GnomePolicyActivation>,
}
struct Both(Arc<dyn PortalInputPolicy>, Arc<dyn PortalInputPolicy>);
impl PortalInputPolicy for Both {
    fn input_available(&self) -> bool {
        self.0.input_available() && self.1.input_available() && self.0.input_available()
    }
    fn user_input_active(&self) -> bool {
        self.0.user_input_active() || self.1.user_input_active()
    }
}
impl Monitor {
    pub(super) async fn activate(
        &mut self,
        base: Arc<dyn PortalInputPolicy>,
    ) -> Result<Arc<dyn PortalInputPolicy>, String> {
        let active = self
            .activation
            .take()
            .ok_or("missing native policy activation")?
            .activate()
            .await?;
        // Preserve Host, parent, lock and original session checks; adding the
        // armed guard must never replace any previously established authority.
        Ok(crate::policy::PolicyLease::new(Arc::new(Both(
            base, active,
        ))))
    }
}

impl PortalRegistry {
    /// Explicit GNOME path: requires the visible, separately installed helper.
    /// Missing/disabled helper fails closed, never falls back to XWayland. This
    /// does not opt the App into native Wayland or install/enable the helper.
    pub fn select_parented_gnome_for_authorization<P, F>(
        &self,
        options: PortalOptions,
        generation: u64,
        target_generation: u64,
        ticket: &AuthorizationTicket,
        prepare: P,
    ) -> Result<PortalSelection, String>
    where
        P: FnOnce() -> F + Send + 'static,
        F: Future<Output = Result<Box<dyn NativeParent>, String>> + Send + 'static,
    {
        self.select_owned(
            options,
            SelectionOwner {
                generation,
                target_generation,
                preparation: Some(ticket.clone()),
                parent: Some(Box::new(move || Box::pin(prepare()))),
                policy: Some(Box::new(|host| {
                    Box::pin(GnomeNativePolicyWatch::connect_for_consent(host))
                })),
            },
            PortalSession::start,
        )
    }
}

pub(super) async fn prepare(
    entry: &Entry,
    options: &mut PortalOptions,
    stop: &mut watch::Receiver<bool>,
    prepare: PolicyPreparation,
    base: Arc<dyn PortalInputPolicy>,
    retained: &mut Option<Monitor>,
) -> Result<Arc<dyn PortalInputPolicy>, String> {
    let deadline = tokio::time::Instant::now() + options.timeout;
    let mut pending = prepare(base.clone());
    // Keep the exact in-flight initializer even after cancellation; the watch
    // it yields is owned and joined, never replaced by a speculative reconnect.
    let (result, interrupted) = tokio::select! {
        biased;
        _ = stop.changed() => { entry.close(); (pending.await, Some("native policy preparation cancelled")) },
        _ = entry.cancellation() => { entry.close(); (pending.await, Some("native policy authorization cancelled")) },
        _ = crate::policy::revoked(base.as_ref()) => { entry.close(); (pending.await, Some("Host policy revoked during native policy preparation")) },
        _ = tokio::time::sleep_until(deadline) => { entry.close(); (pending.await, Some("native policy preparation deadline")) },
        result = &mut pending => (result, None),
    };
    let (watch, activation) = result?;
    let consent = activation.consent_policy();
    let (send, receive) = oneshot::channel();
    *retained = Some(Monitor {
        stop: Some(send),
        task: tokio::spawn(watch.run_until(receive)),
        activation: Some(activation),
    });
    if let Some(reason) = interrupted {
        return Err(reason.into());
    }
    loop {
        if *stop.borrow() || entry.revoked() {
            return Err("native policy preparation revoked".into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("native policy monitor readiness deadline".into());
        }
        if retained.as_ref().is_none_or(|m| m.task.is_finished()) {
            return Err("native policy monitor ended before readiness".into());
        }
        if consent.input_available() && !consent.user_input_active() {
            break;
        }
        tokio::select! {
            _ = stop.changed() => {},
            _ = entry.cancellation() => {},
            _ = tokio::time::sleep(Duration::from_millis(5)) => {},
        }
    }
    // Do not wrap the unpolled watch in a sticky lease: false during startup is
    // not revocation. Install only after all original watchers are truly ready.
    let policy: Arc<dyn PortalInputPolicy> =
        crate::policy::PolicyLease::new(Arc::new(Both(base, consent)));
    *entry
        .monitored_policy
        .lock()
        .map_err(|_| "native policy slot poisoned")? = Some(policy.clone());
    options.timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
    options.validate()?;
    Ok(policy)
}

pub(super) async fn retire(monitor: Option<Monitor>) -> Result<(), String> {
    if let Some(mut monitor) = monitor {
        drop(monitor.activation.take());
        if let Some(stop) = monitor.stop.take() {
            let _ = stop.send(());
        }
        // A normal policy error is revocation, not a lost owner. Panic/cancel
        // fails the ownership proof; do not report Closed/idle in that case.
        let _revocation = monitor
            .task
            .await
            .map_err(|e| format!("native policy owner completion unproven: {e}"))?;
    }
    Ok(())
}
