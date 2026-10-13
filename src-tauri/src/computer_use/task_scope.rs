//! Host-only scope capture. The original handle outlives a cancelled UI waiter.
use super::ComputerUseBroker;
use grok_computer_use_core::broker::{
    AccessApprovalReady, HostScopeSelectionCall, HostScopeSelectionCancel,
};
use std::sync::{Arc, OnceLock};

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskScopeTarget {
    pub(crate) surface: String,
    pub(crate) target_id: String,
}

impl TaskScopeTarget {
    pub(crate) fn parse(targets: &[Self]) -> Result<Vec<(super::SurfaceKind, &str)>, String> {
        if targets.is_empty()
            || targets.len() > grok_computer_use_core::task_consent::MAX_SCOPE_MEMBERS
        {
            return Err("Computer Use requires a bounded explicit selection".into());
        }
        let mut parsed = Vec::with_capacity(targets.len());
        for target in targets {
            let surface = super::SurfaceKind::from_wire(&target.surface)
                .map_err(|_| "Computer Use selection surface unavailable")?;
            if surface.as_wire() != target.surface {
                return Err("Computer Use selection surface unavailable".into());
            }
            if surface != super::SurfaceKind::Desktop {
                return Err("Computer Use task scope producer unavailable for this surface".into());
            }
            if target.target_id.is_empty()
                || target.target_id.trim() != target.target_id
                || target.target_id.len() > 512
                || grok_computer_use_core::broker::SurfaceRouter::classify_target_id(
                    &target.target_id,
                ) != surface
                || parsed.iter().any(|(_, id)| *id == target.target_id)
            {
                return Err("Computer Use selection rejected".into());
            }
            parsed.push((surface, target.target_id.as_str()));
        }
        Ok(parsed)
    }
}

type Workers = super::owned_tasks::OwnedTasks<Result<AccessApprovalReady, String>>;

fn product_workers() -> Arc<Workers> {
    static WORKERS: OnceLock<Arc<Workers>> = OnceLock::new();
    WORKERS.get_or_init(|| Arc::new(Workers::default())).clone()
}

pub(super) fn stop() {
    product_workers().stop();
}
pub(super) async fn join() -> Result<(), String> {
    product_workers().join().await
}

/// Retained through final approval; cancellation also fences a reservation made
/// by this picker. It cannot act on a replacement picker or committed grant.
pub(crate) struct SelectionCall {
    cancel: HostScopeSelectionCancel,
}

impl SelectionCall {
    pub(crate) fn new(selection: &HostScopeSelectionCall) -> Self {
        Self {
            cancel: selection.cancel_handle(),
        }
    }
}

impl Drop for SelectionCall {
    fn drop(&mut self) {
        self.cancel.fence();
    }
}

pub(crate) async fn reserve(
    broker: Arc<ComputerUseBroker>,
    token: String,
    selection: HostScopeSelectionCall,
) -> Result<AccessApprovalReady, String> {
    let runtime = broker.clone();
    reserve_with(broker, token, selection, product_workers(), move || {
        super::execution_runtime::prepare_scope_selection(&runtime, super::SurfaceKind::Desktop)
    })
    .await
}

async fn reserve_with(
    broker: Arc<ComputerUseBroker>,
    token: String,
    selection: HostScopeSelectionCall,
    workers: Arc<Workers>,
    prepare: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<AccessApprovalReady, String> {
    let fence = selection.cancel_handle();
    workers
        .run(
            async move {
                tauri::async_runtime::spawn_blocking(move || {
                    if !selection.is_current() {
                        return Err("Computer Use selection retired".into());
                    }
                    prepare()?;
                    let captured = selection
                        .capture()
                        .map_err(|_| "Computer Use selected instance unavailable")?;
                    broker
                        .reserve_selected_access_approval(&token, captured)
                        .map_err(|_| "Computer Use selection retired or changed".into())
                })
                .await
                .map_err(|_| "Computer Use selection worker did not join".to_string())?
            },
            move || fence.fence(),
        )
        .await?
}

#[cfg(test)]
mod tests;
