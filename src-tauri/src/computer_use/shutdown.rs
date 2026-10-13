//! Bounded Computer Use cleanup before a cooperative App exit.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::session_manager::{McpShutdownReport, SessionManager};

const EXIT_IDLE: u8 = 0;
const EXIT_RUNNING: u8 = 1;
const EXIT_COMPLETE: u8 = 2;
const APP_EXIT_CATALOG_BUDGET: Duration = Duration::from_millis(2_500);

static EXIT_STATE: AtomicU8 = AtomicU8::new(EXIT_IDLE);

/// Probe process uses the same original worker joins as cooperative App exit.
#[cfg(feature = "computer-use-probe")]
pub(crate) async fn join_request_probe_workers() -> Result<(), String> {
    super::consent_watch::stop();
    super::task_access::stop();
    super::task_scope::stop();
    super::task_picker::stop();
    super::task_events::stop();
    super::task_controls::stop();
    super::feature_lifecycle::stop();
    // A failure in one registry must not skip joining the other owners.
    let results = [
        super::consent_watch::join().await,
        super::task_access::join().await,
        super::task_scope::join().await,
        super::task_picker::join().await,
        super::task_events::join().await,
        super::task_controls::join().await,
        super::feature_lifecycle::join().await,
    ];
    let errors = results
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Immediately deny new dispatch while the event loop is still deciding
/// whether it can exit. Remote ACP cleanup is awaited separately.
pub(crate) fn fence_all() {
    super::consent_watch::stop();
    super::task_access::stop();
    super::task_scope::stop();
    super::task_picker::stop();
    super::task_events::stop();
    super::task_controls::stop();
    super::feature_lifecycle::stop();
    if let Some(broker) = super::global_broker() {
        broker.set_feature_enabled(false);
    }
    if let Err(error) = super::sessions::fence_all_checked() {
        tracing::warn!(%error, "Computer Use authority fence during App shutdown failed");
    }
}

pub(crate) async fn shutdown_with_catalog_barrier(
    manager: &SessionManager,
    budget: Duration,
) -> McpShutdownReport {
    fence_all();
    manager.stop_computer_use_dispatcher();
    let mut report = manager.shutdown_computer_use_catalogs(budget).await;
    if let Err(error) = manager.join_computer_use_dispatcher().await {
        report.errors.push(error);
    }
    if let Err(error) = super::consent_watch::join().await {
        report.errors.push(error);
    }
    if let Err(error) = super::task_access::join().await {
        report.errors.push(error);
    }
    if let Err(error) = super::task_scope::join().await {
        report.errors.push(error);
    }
    if let Err(error) = super::task_picker::join().await {
        report.errors.push(error);
    }
    if let Err(error) = super::task_events::join().await {
        report.errors.push(error);
    }
    if let Err(error) = super::task_controls::join().await {
        report.errors.push(error);
    }
    if let Err(error) = super::feature_lifecycle::join().await {
        report.errors.push(error);
    }
    if report.is_clean() {
        tracing::info!(
            attempted = report.attempted,
            "Computer Use App shutdown catalog barrier settled"
        );
    } else {
        tracing::warn!(
            attempted = report.attempted,
            settled = report.settled,
            errors = ?report.errors,
            "Computer Use App shutdown catalog barrier incomplete"
        );
    }
    finish_local_cleanup(report, super::shutdown_product_checked).await
}

async fn finish_local_cleanup(
    mut report: McpShutdownReport,
    cleanup: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> McpShutdownReport {
    let cleanup = tauri::async_runtime::spawn_blocking(cleanup).await;
    match cleanup {
        Ok(Ok(())) => {}
        Ok(Err(error)) => report.errors.push(format!("local resources: {error}")),
        Err(error) => report.errors.push(format!("local shutdown task: {error}")),
    }
    report
}

/// Start the cooperative shutdown once and tell the caller whether it must
/// prevent this ExitRequested event. The follow-up `app.exit` is allowed only
/// after the bounded barrier reaches `EXIT_COMPLETE`.
pub(crate) fn intercept_exit_requested(app: &AppHandle, code: Option<i32>) -> bool {
    match EXIT_STATE.load(Ordering::SeqCst) {
        EXIT_COMPLETE => return false,
        EXIT_RUNNING => return true,
        _ => {}
    }
    if EXIT_STATE
        .compare_exchange(EXIT_IDLE, EXIT_RUNNING, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return EXIT_STATE.load(Ordering::SeqCst) != EXIT_COMPLETE;
    }

    fence_all();
    let manager = app
        .try_state::<Arc<SessionManager>>()
        .map(|state| Arc::clone(state.inner()));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(manager) = manager {
            let _ = shutdown_with_catalog_barrier(&manager, APP_EXIT_CATALOG_BUDGET).await;
        } else {
            tracing::warn!("Computer Use App shutdown has no managed SessionManager");
            let _ = super::consent_watch::join().await;
            let _ = super::task_access::join().await;
            let _ = super::task_scope::join().await;
            let _ = super::task_picker::join().await;
            let _ = super::task_events::join().await;
            let _ = super::task_controls::join().await;
            let _ = super::feature_lifecycle::join().await;
            let _ = tauri::async_runtime::spawn_blocking(super::shutdown_product).await;
        }
        EXIT_STATE.store(EXIT_COMPLETE, Ordering::SeqCst);
        app.exit(code.unwrap_or(0));
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> McpShutdownReport {
        McpShutdownReport {
            attempted: 2,
            settled: 2,
            errors: Vec::new(),
        }
    }

    #[tokio::test]
    async fn local_resource_failure_cannot_be_hidden_by_clean_catalogs() {
        let report = finish_local_cleanup(clean(), || Err("worker still owned".into())).await;
        assert!(!report.is_clean());
        assert_eq!(report.attempted, 2);
        assert_eq!(report.settled, 2);
        assert_eq!(report.errors, vec!["local resources: worker still owned"]);
    }

    #[tokio::test]
    async fn blocking_cleanup_panic_is_failure_not_restart_permission() {
        let report = finish_local_cleanup(clean(), || panic!("owned cleanup panicked")).await;
        assert!(!report.is_clean());
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].starts_with("local shutdown task:"));
    }

    #[tokio::test]
    async fn local_success_preserves_prior_catalog_errors_and_clean_success() {
        let mut original = clean();
        original.settled = 1;
        original.errors.push("catalog pending".into());
        let report = finish_local_cleanup(original, || Ok(())).await;
        assert!(!report.is_clean());
        assert_eq!(report.settled, 1);
        assert_eq!(report.errors, vec!["catalog pending"]);
        assert!(finish_local_cleanup(clean(), || Ok(())).await.is_clean());
    }
}
