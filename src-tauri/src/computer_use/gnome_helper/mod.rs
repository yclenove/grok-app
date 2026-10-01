//! Explicit desktop settings only. Readiness is a diagnostic, never an input
//! grant. Default builds cannot mutate the helper. No automatic installation.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
mod bundle;
#[cfg(target_os = "linux")]
mod lock;
#[cfg(target_os = "linux")]
mod native;

static BUSY: AtomicBool = AtomicBool::new(false);
struct Operation;
impl Operation {
    fn acquire() -> Result<Self, String> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "computer_use_helper_busy".into())
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HelperAction {
    Install,
    Repair,
    Enable,
    Disable,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HelperStatus {
    pub available: bool,
    pub busy: bool,
    pub feature_enabled: bool,
    pub state: &'static str,
    pub shell_version: Option<String>,
    pub installation: Option<String>,
    pub actions: Vec<HelperAction>,
}
impl HelperStatus {
    fn unavailable(state: &'static str) -> Self {
        Self {
            available: false,
            busy: BUSY.load(Ordering::Acquire),
            feature_enabled: super::feature::feature_enabled(),
            state,
            shell_version: None,
            installation: None,
            actions: Vec::new(),
        }
    }
}

pub(crate) async fn status() -> Result<HelperStatus, String> {
    if !cfg!(all(
        target_os = "linux",
        feature = "computer-use-wayland-preview"
    )) {
        return Ok(HelperStatus::unavailable("unavailable"));
    }
    #[cfg(target_os = "linux")]
    {
        native::status().await
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(HelperStatus::unavailable("unavailable"))
    }
}

pub(crate) async fn act(action: HelperAction) -> Result<HelperStatus, String> {
    if !cfg!(all(
        target_os = "linux",
        feature = "computer-use-wayland-preview"
    )) {
        return Err("computer_use_helper_unavailable".into());
    }
    // The owned task, cross-process file lock and feature-transition lock all
    // outlive a cancelled IPC waiter or unmounted settings panel. No retry.
    spawn_owned(async move {
        #[cfg(target_os = "linux")]
        {
            native::act(action).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = action;
            Err("computer_use_helper_unavailable".to_string())
        }
    })?
    .await
    .map_err(|e| e.to_string())??;
    status().await
}

fn spawn_owned(
    work: impl std::future::Future<Output = Result<(), String>> + Send + 'static,
) -> Result<tauri::async_runtime::JoinHandle<Result<(), String>>, String> {
    let operation = Operation::acquire()?;
    Ok(tauri::async_runtime::spawn(async move {
        let _operation = operation;
        work.await
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancelled_waiter_retains_original_mutation_owner_until_completion() {
        let (release, held) = tokio::sync::oneshot::channel();
        let task = spawn_owned(async move { held.await.map_err(|e| e.to_string()) }).unwrap();
        let waiter = tokio::spawn(task);
        waiter.abort();
        let _ = waiter.await;
        assert!(Operation::acquire().is_err());
        assert!(BUSY.load(Ordering::Acquire));
        release.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while BUSY.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(Operation::acquire().unwrap());
        assert!(!BUSY.load(Ordering::Acquire));
    }
    #[cfg(not(all(target_os = "linux", feature = "computer-use-wayland-preview")))]
    #[tokio::test]
    async fn default_or_other_platform_never_mutates() {
        assert!(!status().await.unwrap().available);
        for action in [
            HelperAction::Install,
            HelperAction::Repair,
            HelperAction::Enable,
            HelperAction::Disable,
        ] {
            assert!(act(action).await.is_err());
        }
    }
}
