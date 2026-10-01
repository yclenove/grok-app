//! Native UI export ownership belongs to the selection, including queued work.
use super::*;
use std::{future::Future, pin::Pin};

pub(super) type ParentPreparation = Box<
    dyn FnOnce() -> Pin<Box<dyn Future<Output = Result<Box<dyn NativeParent>, String>> + Send>>
        + Send,
>;

pub(crate) struct SelectionOwner {
    pub generation: u64,
    pub target_generation: u64,
    pub preparation: Option<AuthorizationTicket>,
    pub parent: Option<ParentPreparation>,
    pub policy: Option<super::native_policy::PolicyPreparation>,
}

struct ParentPolicy {
    base: Arc<dyn PortalInputPolicy>,
    parent: Arc<dyn NativeParent>,
}

impl PortalInputPolicy for ParentPolicy {
    fn input_available(&self) -> bool {
        !self.parent.is_revoked() && self.base.input_available() && !self.parent.is_revoked()
    }

    fn user_input_active(&self) -> bool {
        self.parent.is_revoked() || self.base.user_input_active() || self.parent.is_revoked()
    }
}

pub(super) async fn prepare(
    entry: &Entry,
    options: &mut PortalOptions,
    stop: &mut watch::Receiver<bool>,
    prepare: ParentPreparation,
    policy: Arc<dyn PortalInputPolicy>,
) -> Result<Arc<dyn PortalInputPolicy>, String> {
    let ticket = entry
        .preparation
        .as_ref()
        .ok_or("missing parent authorization")?;
    ticket.check_preparation()?;
    if *stop.borrow() {
        return Err("native parent preparation cancelled".into());
    }
    let deadline = tokio::time::Instant::now() + options.timeout;
    // Dispatch only here, after insertion. Never drop this future on timeout:
    // the GTK queue could already have created an export not yet delivered.
    let mut pending = prepare();
    let (result, interrupted) = tokio::select! {
        biased;
        _ = stop.changed() => {
            entry.close();
            (pending.await, Some("native parent preparation cancelled"))
        }
        _ = entry.cancellation() => {
            entry.close();
            (pending.await, Some("native parent authorization cancelled"))
        }
        _ = crate::policy::revoked(policy.as_ref()) => {
            entry.close();
            (pending.await, Some("Host policy revoked during native parent preparation"))
        }
        _ = tokio::time::sleep_until(deadline) => {
            entry.close();
            (pending.await, Some("native parent preparation deadline"))
        }
        result = &mut pending => (result, None),
    };
    let parent: Arc<dyn NativeParent> = Arc::from(result?);
    // Retain BEFORE validation, even if a faulty Host hands back the wrong
    // preparation or the UI delivers after revocation. Retirement must join it.
    *entry
        .parent
        .lock()
        .map_err(|_| "native parent slot poisoned")? = Some(parent.clone());
    if let Some(reason) = interrupted {
        return Err(reason.into());
    }
    if !ticket.same_preparation(parent.authorization()) {
        return Err("native parent belongs to a different authorization".into());
    }
    loop {
        ticket.check_preparation()?;
        if *stop.borrow()
            || parent.is_revoked()
            || !policy.input_available()
            || policy.user_input_active()
        {
            return Err("native parent revoked before portal consent".into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("native parent preparation deadline".into());
        }
        if let Some(handle) = parent.handle()? {
            if handle.is_empty() {
                return Err("native parent returned an empty handle".into());
            }
            options.parent_window = handle;
            options.timeout = deadline.saturating_duration_since(tokio::time::Instant::now());
            options.validate()?;
            if !policy.input_available() || policy.user_input_active() {
                return Err("Host policy revoked before parented portal consent".into());
            }
            return Ok(Arc::new(ParentPolicy {
                base: policy,
                parent,
            }));
        }
        tokio::select! {
            _ = stop.changed() => {},
            _ = entry.cancellation() => {},
            _ = tokio::time::sleep(Duration::from_millis(5)) => {},
        }
    }
}

pub(super) async fn retire(entry: &Entry) -> Result<(), String> {
    let parent = entry
        .parent
        .lock()
        .map_err(|_| "native parent slot poisoned")?
        .clone();
    if let Some(parent) = parent {
        entry.close();
        parent.request_close();
        // No timeout, remote Closed, or dropped receiver can stand in for the
        // original UI owner's release. A stopped GTK loop stays occupied.
        while !parent.is_closed() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    Ok(())
}
