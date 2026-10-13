//! First-use Host transaction. Only an opaque Broker reservation enters here.
use super::feature_lifecycle::{choice_revision, transition_lock};
use super::feature_preferences::{FeaturePreferences, PreferenceReceipt};
use super::ComputerUseBroker;
use grok_computer_use_core::broker::{AccessApproval, AccessApprovalCancel};
use grok_computer_use_core::task_consent::CommitOutcome;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
mod workers;

/// Host-only, synchronous authority publication. Never serialized as a grant.
pub(crate) type ApprovalPublication =
    Arc<dyn Fn(&mut AccessApproval) -> Result<CommitOutcome, String> + Send + Sync>;

pub(super) fn stop() {
    workers::product().stop();
}
pub(super) async fn join() -> Result<(), String> {
    workers::product().join().await
}

struct ApprovalCall {
    cancel: AccessApprovalCancel,
    cancelled: Arc<AtomicBool>,
    completed: bool,
}

impl Drop for ApprovalCall {
    fn drop(&mut self) {
        if !self.completed {
            self.cancelled.store(true, Ordering::SeqCst);
            self.cancel.fence();
        }
    }
}

/// An explicit card/picker decision supplies an approval from the exact ACP
/// transport. Native targets already have Bound; an isolated task browser's
/// resource reservation establishes Bound during owned execution preparation.
pub(crate) async fn approve(
    broker: Arc<ComputerUseBroker>,
    approval: AccessApproval,
    publication: ApprovalPublication,
) -> Result<CommitOutcome, String> {
    let surface = approval.surface();
    let runtime_broker = Arc::clone(&broker);
    approve_with_publication(
        broker,
        approval,
        FeaturePreferences::product(),
        workers::product(),
        publication,
        move || super::execution_runtime::prepare(&runtime_broker, surface),
    )
    .await
}

#[cfg(test)]
pub(super) async fn approve_with<F>(
    broker: Arc<ComputerUseBroker>,
    approval: AccessApproval,
    preferences: FeaturePreferences,
    workers: Arc<workers::Workers>,
    prepare_runtime: F,
) -> Result<CommitOutcome, String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    approve_with_publication(
        broker,
        approval,
        preferences,
        workers,
        Arc::new(|approval| {
            approval
                .commit_authority()
                .map_err(|_| "Computer Use final commit rejected".into())
        }),
        prepare_runtime,
    )
    .await
}

pub(super) async fn approve_with_publication<F>(
    broker: Arc<ComputerUseBroker>,
    approval: AccessApproval,
    preferences: FeaturePreferences,
    workers: Arc<workers::Workers>,
    publication: ApprovalPublication,
    prepare_runtime: F,
) -> Result<CommitOutcome, String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut call = ApprovalCall {
        cancel: approval.cancel_handle(),
        cancelled: Arc::clone(&cancelled),
        completed: false,
    };
    let revision = choice_revision();
    let worker_cancel = approval.cancel_handle();
    let worker_cancelled = Arc::clone(&cancelled);
    // The worker owns persistence, native preparation and eventual rollback
    // even when the Tauri caller is cancelled. The caller's Drop only fences.
    let result = workers
        .run(
            async move {
                let mut approval = Some(approval);
                let mut receipt = None;
                let result = run_approval(
                    &mut approval,
                    &preferences,
                    &mut receipt,
                    revision,
                    &cancelled,
                    publication,
                    prepare_runtime,
                )
                .await;
                if result.is_err() {
                    let _transition = transition_lock().lock().await;
                    if let Some(mut approval) = approval {
                        approval.abort();
                    }
                    super::set_feature_enabled(broker.feature_enabled());
                    if choice_revision() == revision && !broker.feature_enabled() {
                        if let Some(receipt) = receipt {
                            tauri::async_runtime::spawn_blocking(move || {
                                preferences.rollback(&receipt)
                            })
                            .await
                            .map_err(|_| "Computer Use preference rollback did not join")??;
                        }
                    }
                }
                result
            },
            move || {
                worker_cancelled.store(true, Ordering::SeqCst);
                worker_cancel.fence();
            },
        )
        .await?;
    call.completed = true;
    result
}

async fn run_approval<F>(
    approval: &mut Option<AccessApproval>,
    preferences: &FeaturePreferences,
    receipt: &mut Option<PreferenceReceipt>,
    revision: u64,
    cancelled: &AtomicBool,
    publication: ApprovalPublication,
    prepare_runtime: F,
) -> Result<CommitOutcome, String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    let current = |approval: &AccessApproval| {
        !cancelled.load(Ordering::SeqCst) && choice_revision() == revision && approval.is_current()
    };
    {
        let _transition = transition_lock().lock().await;
        let pending = approval.as_mut().expect("owned approval");
        if !current(pending) {
            return Err("Computer Use approval retired".into());
        }
        let expected = pending.initially_enabled();
        let store = preferences.clone();
        *receipt = tauri::async_runtime::spawn_blocking(move || store.enable(expected))
            .await
            .map_err(|_| "Computer Use preference writer did not join")??;
        if !current(pending) {
            return Err("Computer Use approval retired".into());
        }
        if let Err(error) = pending.publish_feature() {
            tracing::warn!(
                stage = "feature-publication",
                surface = pending.surface().as_wire(),
                error = %error,
                "computer-use approval transaction failed"
            );
            return Err("Computer Use approval retired".into());
        }
        super::set_feature_enabled(true);
    }
    let mut pending = approval.take().expect("owned approval");
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        let surface = pending.surface();
        let result = if !pending.is_current() {
            Err("Computer Use approval retired".into())
        } else if let Err(error) = prepare_runtime() {
            tracing::warn!(
                stage = "runtime-preparation",
                surface = surface.as_wire(),
                error = %error,
                "computer-use approval transaction failed"
            );
            Err(error)
        } else if let Err(error) = pending.prepare_execution() {
            tracing::warn!(
                stage = "execution-preparation",
                surface = surface.as_wire(),
                error = %error,
                "computer-use approval transaction failed"
            );
            Err("Computer Use execution preparation failed".into())
        } else if let Err(error) = pending.prepare_publication() {
            tracing::warn!(
                stage = "publication-preparation",
                surface = surface.as_wire(),
                error = %error,
                "computer-use approval transaction failed"
            );
            Err("Computer Use execution preparation failed".into())
        } else {
            Ok(())
        };
        (pending, result)
    })
    .await
    .map_err(|_| "Computer Use execution preparation did not join")?;
    *approval = Some(prepared.0);
    prepared.1?;
    let _transition = transition_lock().lock().await;
    if !current(approval.as_ref().expect("owned approval")) {
        return Err("Computer Use approval retired".into());
    }
    let store = preferences.clone();
    let receipt = receipt.clone();
    let mut pending = approval.take();
    let committed = tauri::async_runtime::spawn_blocking(move || {
        let result = store.with_owned(receipt.as_ref(), || {
            publication(pending.as_mut().expect("owned approval"))
        });
        (pending, result)
    })
    .await
    .map_err(|_| "Computer Use final commit did not join")?;
    *approval = committed.0;
    committed.1
}

#[cfg(test)]
mod tests;
