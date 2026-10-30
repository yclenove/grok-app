//! Live accessibility snapshot and semantic actions. Portable tests never call this.

use gcu_core::driver::{Cancel, Delivery, DriverAction, DriverError, Result};

#[cfg(any(test, target_os = "macos"))]
use gcu_core::model::NativeHandle;

use crate::driver::MacosDriver;

#[cfg(target_os = "macos")]
use crate::ax;

#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use gcu_core::{driver::TreeOpts, model::RawTree};

#[cfg(target_os = "macos")]
use crate::ax::ErasedToggle;
#[cfg(target_os = "macos")]
use crate::discovery::TreeTarget;
#[cfg(any(test, target_os = "macos"))]
use crate::scope_os::{Evidence, JobScope};

impl MacosDriver {
    #[cfg(target_os = "macos")]
    pub(crate) async fn snapshot_live(
        &self,
        target: &TreeTarget,
        opts: &TreeOpts,
        cancel: &Cancel,
        remaining: Duration,
    ) -> Result<RawTree> {
        let Some(thread) = self.ax.clone() else {
            return Err(DriverError::Native {
                message: "accessibility tree source is not connected".into(),
                maybe_executed: false,
            });
        };
        if ax::needs_enhanced_accessibility(&target.bundle_id, &target.app_name) {
            let session_scoped = self.evidence.session_scoped();
            let store = if session_scoped {
                crate::real::AxToggleStore::without_process_writes(Arc::clone(&thread))
            } else {
                crate::real::AxToggleStore::new(Arc::clone(&thread))
            };
            let toggles = Arc::new(ax::AccessibilityToggles::new(store));
            let enabling = Arc::clone(&toggles);
            let pid = target.pid;
            let enabled = tokio::task::spawn_blocking(move || {
                ax::toggles::arm_for_observation(enabling.as_ref(), pid, session_scoped)
            })
            .await
            .map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
            enabled?;
            if !session_scoped {
                self.track_toggles(ArcToggle(toggles));
            }
        }
        let Some(index) = target.ax_index else {
            return Ok(RawTree {
                root: None,
                incomplete: None,
            });
        };
        let source = crate::real::RealAxSource::open(thread, target.pid, index).await?;
        source.set_text_limit(opts.text_limit);
        let mut walk = opts.clone();
        walk.timeout = remaining.min(ax::TREE_BUILD_TIMEOUT);
        let root = source.root();
        ax::build_raw_tree(&source, &root, &walk, cancel, target.scale).await
    }

    pub(crate) async fn perform_live_action(
        &self,
        req: &DriverAction,
        cancel: &Cancel,
    ) -> Result<Delivery> {
        #[cfg(target_os = "macos")]
        if self.live_ax.load(Ordering::Acquire) {
            if let Some(thread) = &self.ax {
                let evidence = Arc::clone(&self.evidence);
                let source = crate::real::RealAxSource::attach(
                    Arc::clone(thread),
                    Some(Arc::clone(&evidence)),
                );
                source.set_scope(ax_job_scope(evidence.as_ref(), req)?);
                return ax::perform(&source, req, cancel, gcu_core::driver::DRIVER_DEADLINE).await;
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (req, cancel);
        Err(action_source_missing())
    }
}

/// Scope for one semantic action.
///
/// Session scope and the pin are read once, into `admitted`. Later clears of
/// the pin slot are ignored. A scoped session with no binding stays scoped
/// and refuses; it does not become [`JobScope::unscoped`], including when the
/// live adapter is not enforcing. Memory only, aside from the element lookup.
/// No extra timeout or cancellation.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn ax_job_scope(evidence: &Evidence, req: &DriverAction) -> Result<JobScope> {
    let admitted = JobScope::admit(evidence);
    #[cfg(test)]
    evidence.run_admission_barrier();
    if admitted.is_scoped() {
        let Some(binding) = admitted.expected().cloned() else {
            return Ok(JobScope::scoped(None));
        };
        let handle = action_element(req).ok_or_else(crate::scope_os::not_proved)?;
        let live = evidence
            .element_handle(handle)
            .ok_or_else(crate::scope_os::not_proved)?;
        if live.window() != &binding {
            return Err(crate::scope_os::not_proved());
        }
        return Ok(JobScope::scoped(Some(binding)));
    }
    if evidence.enforcing() {
        let handle = action_element(req).ok_or_else(crate::scope_os::not_proved)?;
        let binding = evidence
            .element_handle(handle)
            .ok_or_else(crate::scope_os::not_proved)?;
        return Ok(JobScope::scoped(Some(binding.window().clone())));
    }
    Ok(JobScope::unscoped())
}

#[cfg(any(test, target_os = "macos"))]
fn action_element(req: &DriverAction) -> Option<&NativeHandle> {
    match req {
        DriverAction::Press { el }
        | DriverAction::SecondaryAction { el, .. }
        | DriverAction::SetValue { el, .. }
        | DriverAction::SelectText { el, .. }
        | DriverAction::ScrollElement { el, .. } => Some(el),
        _ => None,
    }
}

fn action_source_missing() -> DriverError {
    DriverError::Native {
        message: "accessibility action source is not connected".into(),
        maybe_executed: false,
    }
}

/// `Arc` wrapper so tracked toggles restore when the driver drops them.
#[cfg(target_os = "macos")]
struct ArcToggle<S: ax::ToggleStore>(Arc<ax::AccessibilityToggles<S>>);

#[cfg(target_os = "macos")]
impl<S: ax::ToggleStore + 'static> ErasedToggle for ArcToggle<S> {
    fn restore(&self) {
        self.0.restore();
    }
}
