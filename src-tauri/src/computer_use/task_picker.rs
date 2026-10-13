//! Original discovery/reservation handles survive a cancelled UI command.
use super::ComputerUseBroker;
use grok_computer_use_core::broker::{
    AccessApprovalReady, HostScopePicker, HostScopePickerCall, HostScopePickerCancel,
    HostScopeSelection,
};
use std::sync::{Arc, OnceLock};

type DiscoveryWorkers = super::owned_tasks::OwnedTasks<Result<HostScopePicker, String>>;
type ReservationWorkers = super::owned_tasks::OwnedTasks<Result<AccessApprovalReady, String>>;

fn discovery_workers() -> Arc<DiscoveryWorkers> {
    static WORKERS: OnceLock<Arc<DiscoveryWorkers>> = OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(DiscoveryWorkers::default()))
        .clone()
}
fn reservation_workers() -> Arc<ReservationWorkers> {
    static WORKERS: OnceLock<Arc<ReservationWorkers>> = OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(ReservationWorkers::default()))
        .clone()
}

#[cfg(all(test, windows))]
pub(crate) fn reservation_owners_for_test() -> usize {
    reservation_workers().owner_count_for_test()
}

#[cfg(all(test, windows))]
pub(crate) fn discovery_owners_for_test() -> usize {
    discovery_workers().owner_count_for_test()
}

#[cfg(all(test, windows))]
pub(crate) fn original_discovery_entry_for_test() -> Result<uuid::Uuid, String> {
    discovery_workers().singleton_entry_for_test()
}

#[cfg(all(test, windows))]
pub(crate) fn original_reservation_entry_for_test() -> Result<uuid::Uuid, String> {
    reservation_workers().singleton_entry_for_test()
}

#[cfg(all(test, windows))]
pub(crate) async fn join_abandoned_reservation_for_test(
    id: uuid::Uuid,
) -> Result<Result<AccessApprovalReady, String>, String> {
    reservation_workers().join_abandoned_for_test(id).await
}

#[cfg(all(test, windows))]
pub(crate) async fn join_abandoned_discovery_for_test(
    id: uuid::Uuid,
) -> Result<Result<HostScopePicker, String>, String> {
    discovery_workers().join_abandoned_for_test(id).await
}

struct WorkerCall {
    cancel: HostScopePickerCancel,
    completed: bool,
}

impl Drop for WorkerCall {
    fn drop(&mut self) {
        if !self.completed {
            self.cancel.fence();
        }
    }
}

pub(super) fn stop() {
    discovery_workers().stop();
    reservation_workers().stop();
}
pub(super) async fn join() -> Result<(), String> {
    let discovery = discovery_workers().join().await;
    let reservation = reservation_workers().join().await;
    discovery.and(reservation)
}

pub(crate) fn prepare(
    broker: &ComputerUseBroker,
    surface: super::SurfaceKind,
) -> Result<(), String> {
    super::execution_runtime::prepare_scope_selection(broker, surface)
}

pub(crate) async fn discover(
    broker: Arc<ComputerUseBroker>,
    call: HostScopePickerCall,
) -> Result<HostScopePicker, String> {
    let surface = call.surface();
    discover_with(call, discovery_workers(), move || prepare(&broker, surface)).await
}

async fn discover_with(
    call: HostScopePickerCall,
    workers: Arc<DiscoveryWorkers>,
    prepare: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<HostScopePicker, String> {
    let fence = call.cancel_handle();
    let mut guard = WorkerCall {
        cancel: fence.clone(),
        completed: false,
    };
    let result = workers
        .run(
            async move {
                tauri::async_runtime::spawn_blocking(move || {
                    if !call.is_current() {
                        return Err("Computer Use picker retired".into());
                    }
                    prepare()?;
                    call.discover()
                        .map_err(|_| "Computer Use scope picker unavailable or retired".into())
                })
                .await
                .map_err(|_| "Computer Use picker discovery worker did not join".to_string())?
            },
            move || fence.fence(),
        )
        .await?;
    guard.completed = result.is_ok();
    result
}

pub(crate) async fn reserve(
    broker: Arc<ComputerUseBroker>,
    token: String,
    selection: HostScopeSelection,
    cancel: HostScopePickerCancel,
) -> Result<AccessApprovalReady, String> {
    reserve_with(broker, token, selection, cancel, reservation_workers()).await
}

async fn reserve_with(
    broker: Arc<ComputerUseBroker>,
    token: String,
    selection: HostScopeSelection,
    cancel: HostScopePickerCancel,
    workers: Arc<ReservationWorkers>,
) -> Result<AccessApprovalReady, String> {
    let mut guard = WorkerCall {
        cancel: cancel.clone(),
        completed: false,
    };
    let result = workers
        .run(
            async move {
                tauri::async_runtime::spawn_blocking(move || {
                    broker
                        .reserve_selected_access_approval(&token, selection)
                        .map_err(|error| {
                            tracing::warn!(
                                stage = "scope-reservation",
                                error = %error,
                                "computer-use picker approval failed"
                            );
                            "Computer Use selected window instance retired or changed".into()
                        })
                })
                .await
                .map_err(|_| "Computer Use picker reservation worker did not join".to_string())?
            },
            move || cancel.fence(),
        )
        .await?;
    guard.completed = result.is_ok();
    result
}

#[cfg(test)]
mod tests;
